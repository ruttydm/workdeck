//! Ratatui review canvas.

mod agent_annotations;
mod agent_card_view;
#[cfg(test)]
mod agent_inline_note_parity_tests;
mod agent_inline_note_view;
mod agent_note_geometry;
mod agent_popover;
mod agent_skill_dialog;
mod app_commands;
mod app_host;
mod app_menus;
mod code_cell_view;
mod code_row_layout;
mod code_row_view;
mod color;
mod command_keymap;
mod command_keys;
mod confirm_dialog;
mod copy_selection;
mod current_review_controller;
mod current_review_refresh;
mod cursor_highlight;
mod diff_meta_row_view;
mod diff_row_view;
mod diff_rows;
mod diff_section_body;
mod diff_section_geometry;
mod diff_section_row_plan;
mod diff_section_view;
mod extension_command_controls;
mod extension_commands;
mod extension_current_line;
mod extension_dialog_view;
mod extension_dialogs;
mod extension_navigation;
mod extension_notifications;
mod extension_pane_controller;
mod extension_pane_host;
mod extension_panes;
mod extension_review_events;
pub mod extension_runtime_bridge;
mod extension_trust_controller;
mod extension_trust_prompt;
mod extension_workspace;
mod file_header;
mod file_presentation_controller;
mod file_presentation_rendering;
mod file_render_window;
mod file_section_layout;
mod file_view_geometry;
mod file_view_layouts;
mod file_view_view;
mod help_content;
mod help_dialog;
mod highlight_prefetch;
mod highlighted_diff_runtime;
mod hunk_scroll;
mod ids;
mod interactive_runtime;
mod interactive_session_adapter;
mod job_control;
mod key_routing;
mod keyboard;
mod line_cursors;
mod line_highlight_paint;
mod line_highlights;
mod list_geometry;
mod menu;
mod modal_frame;
mod mouse_capture;
mod mouse_scroll;
mod open_in_editor;
#[cfg(unix)]
mod piped_input;
mod planned_review_row;
mod planned_row_text;
mod public_review;
mod review_render_plan;
mod review_row_geometry;
mod review_state_helpers;
mod row_style;
mod session_review_controller;
mod shutdown;
mod source_controller;
mod source_presentation;
pub use source_presentation::ReviewSourcePresentation;
mod spatial;
mod startup_notices;
mod static_diff_pager;
mod status_bar;
mod synthetic_key_event;
mod terminal_runtime;
mod text;
mod theme;
mod theme_detection;
mod theme_selector_controller;
mod theme_selector_dialog;
mod timed_notice;
mod ui_geometry;
#[cfg(test)]
mod ui_lib_parity_tests;
mod user_note_composer;
mod vertical_scrollbar;
mod view_preference_quit_controller;
mod viewport_anchor;
mod viewport_geometry;
mod viewport_selection;
mod watched_input;

use extension_dialog_view::{
    ExtensionInputDialogPlan, ExtensionSelectDialogPlan, extension_dialog_action_at,
    render_extension_input_dialog_view, render_extension_select_dialog_view, select_item_at,
};
use extension_dialogs::{
    ExtensionConfirmDialog, ExtensionDialogAnswer, ExtensionDialogError, ExtensionDialogQueue,
    ExtensionDialogRequest, ExtensionDialogSettlement, ExtensionInputDialog, ExtensionSelectDialog,
    ExtensionWorkspaceWriteDialog,
};

pub use agent_annotations::*;
pub use agent_card_view::*;
pub use agent_inline_note_view::*;
pub use agent_note_geometry::*;
pub use agent_popover::*;
pub use agent_skill_dialog::*;
pub use app_commands::*;
pub use app_host::*;
pub use app_menus::*;
pub use code_cell_view::*;
pub use code_row_layout::*;
pub use code_row_view::*;
pub use color::*;
pub use command_keymap::*;
pub use command_keys::*;
pub use confirm_dialog::*;
pub use copy_selection::*;
pub use current_review_controller::*;
pub use current_review_refresh::*;
pub use cursor_highlight::*;
pub use diff_meta_row_view::*;
pub use diff_row_view::*;
pub use diff_rows::*;
pub use diff_section_body::*;
pub use diff_section_geometry::*;
pub use diff_section_row_plan::*;
pub use diff_section_view::*;
pub use extension_commands::*;
pub use extension_current_line::*;
pub use extension_navigation::*;
pub use extension_notifications::*;
pub use extension_pane_controller::*;
pub use extension_pane_host::*;
pub use extension_panes::*;
pub use extension_review_events::*;
pub use extension_trust_controller::*;
pub use extension_trust_prompt::*;
pub use extension_workspace::*;
pub use file_header::*;
pub use file_presentation_controller::*;
pub use file_presentation_rendering::*;
pub use file_render_window::*;
pub use file_section_layout::*;
pub use file_view_geometry::*;
pub use file_view_layouts::*;
pub use file_view_view::*;
pub use help_content::*;
pub use help_dialog::*;
pub use highlight_prefetch::{adjacent_highlight_prefetch_ids, highlight_prefetch_ids};
pub use highlighted_diff_runtime::*;
pub use hunk_scroll::*;
pub use ids::*;
pub use interactive_runtime::*;
pub use interactive_session_adapter::*;
pub use job_control::*;
pub use key_routing::*;
pub use keyboard::*;
pub use line_cursors::*;
pub use line_highlight_paint::*;
pub use line_highlights::*;
pub use list_geometry::*;
pub use menu::*;
pub use modal_frame::*;
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
pub use theme_selector_controller::*;
pub use theme_selector_dialog::*;
pub use timed_notice::*;
pub use ui_geometry::*;
pub use user_note_composer::*;
pub use vertical_scrollbar::*;
pub use view_preference_quit_controller::*;
pub use viewport_anchor::*;
pub use viewport_geometry::*;
pub use viewport_selection::*;
pub use watched_input::*;

use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
#[cfg(test)]
use ratatui::Terminal;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use workdeck_core::{
    AgentAnnotation, Changeset, ChangesetSource, DiffFile, DiffLine, DiffLineKind, InputCursorLine,
    InputLayoutMode, NamedCustomThemeConfig, PersistedViewPreferences, ReviewSelection, ReviewSide,
    SidebarVisibility, SourceOrigin, StartupNotice,
};
use workdeck_diff::{
    DIFF_RAIL_PREFIX_WIDTH, HighlightedDiffLine, LanguageMatcher, LanguageRegistration,
    LanguageRegistry, SyntaxToken, TextSegment, clip_segments, expand_diff_tabs,
    find_max_line_number, plan_split_line_pairs, resolve_split_cell_geometry,
    resolve_split_pane_widths as resolve_diff_split_pane_widths, resolve_stack_cell_geometry,
    sanitize_terminal_line, slice_segments_window, word_diff_ranges, wrap_segments,
};
use workdeck_extension_api::{
    ExtensionCommandAvailability, ExtensionCurrentLinePaint as ExtensionCurrentLinePaintContext,
    ExtensionFileSide, ExtensionFileViewContext, ExtensionHostAction, ExtensionKeyEvent,
    ExtensionLayoutMode, ExtensionLifecycleEvent, ExtensionNotification, ExtensionNotificationHub,
    ExtensionNotificationSubscription, ExtensionNotifyType, ExtensionPaintTheme, ExtensionPaneView,
    ExtensionResolvedKeybindings, ExtensionResolvedLayout, ExtensionReviewNote,
    ExtensionWorkspaceReadCompletion, ExtensionWorkspaceWriteCompletion,
    ExtensionWorkspaceWriteResult, FileLanguageGlobTarget, FileLanguageMatcher,
    FileViewModeKeyRequest, FileViewModeLifecycleRequest, KeyRoutingResult,
    KeyboardModeRegistration, PaneActionInvocation, PaneAvailabilityRequest, PaneInputInvocation,
    PanePlacement, PaneRegistration, PaneRenderRequest, Registration, ReviewEvent,
    SessionReloadReason, ViewNode, ViewStyle, WORKDECK_FILES_PANE_KEY, bundled_files_pane,
    extension_pane_size, file_view_unavailable_reason,
};
use workdeck_extension_host::{
    ActiveSessionKeyboardMode, EXTENSION_SHUTDOWN_TIMEOUT,
    ExtensionEventContextProviderInstallation, ExtensionEventContextProviderSlot,
    FileViewSelectionState, HostError, KeyboardModeActionAuthority, KeyboardModeControllerState,
    LineHighlightRefreshResult, LineHighlightsController, LoadedExtension, RegisteredFileView,
    RegisteredKeyboardMode, RegisteredLineHighlighter, create_file_view_input,
    create_file_view_input_snapshot, file_view_mode_failure_message, format_keyboard_mode_failure,
    project_extension_changeset, project_extension_diff_file, reconcile_file_view_epochs,
    reconcile_file_view_selections, registered_file_view_key,
    resolve_loaded_extension_registrations, select_file_view, select_file_view_for_files,
    session_keyboard_mode_display_title, session_keyboard_mode_status_hint,
    session_keyboard_mode_still_valid,
};
use workdeck_review::{
    CommentAnchor, LayoutMode, PlannedFileViewRow, ReviewComment, ReviewGapAddress,
    ReviewLineTarget, ReviewNavigationFile, ReviewNavigationModel, ReviewNoteResolution,
    ReviewRevealAnchor, ReviewRevealNoteCandidate, ReviewRevealRequest, ReviewSelectionMove,
    ReviewSelectionScope, ReviewState, SemanticReviewAnnotationIndex, SemanticReviewSelection,
    VisibleFileViewNote, build_extension_review_snapshot, build_file_view_render_plan,
    plan_expanded_gap, plan_review_selection_move, project_extension_review_notes,
    resolve_review_reveal_note_id, review_annotated_hunk_indices, review_default_hunk_line_target,
    review_expansion_side, review_file_fields_match_filter, review_gap_source_for_file,
    review_leading_gap, review_line_anchor, review_trailing_gap,
};
use workdeck_session::{ReviewSessionServer, default_discovery_directory};
use workdeck_vcs::bundled_vcs_catalog;

use crate::extension_runtime_bridge::{
    ExtensionFileProjectionCache, ExtensionRuntimeBridge, ExtensionRuntimeCommit,
    ExtensionRuntimeNavigation,
};

#[derive(Debug, Clone)]
pub struct ReviewOptions {
    /// Non-serialized provider authority supplied by the composition root.
    pub source_capabilities: Option<workdeck_vcs::VcsSourceCapabilities>,
    /// Identity-bound load presentation; executable source readers remain host-owned.
    pub source_presentation: source_presentation::ReviewSourcePresentation,
    pub layout: LayoutMode,
    /// Initial and most recent explicit sidebar policy; `sidebar` retains the logical open state.
    pub sidebar_visibility: SidebarVisibility,
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
    pub experimental: bool,
    pub show_menu_bar: bool,
    pub copy_decorations: bool,
    /// Existing repository config, otherwise the global config path, for view persistence.
    pub view_preferences_config_path: Option<PathBuf>,
    /// Whether changed persistent view settings require a decision before quitting.
    pub prompt_save_view_preferences: bool,
    /// Extension-owned sessions may explicitly prevent persistence of their temporary view.
    pub transient_view_preferences: bool,
    /// Explicit home used only to shorten the config label in the confirmation dialog.
    pub view_preferences_home_directory: Option<PathBuf>,
    pub theme: AppTheme,
    /// Configured and extension-provided themes retained for the live selector.
    pub custom_themes: Vec<NamedCustomThemeConfig>,
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
    /// Process/session cancellation projected into the renderer without transferring signal ownership.
    pub external_quit_signal: Option<Arc<AtomicBool>>,
}

impl Default for ReviewOptions {
    fn default() -> Self {
        Self {
            source_presentation: source_presentation::ReviewSourcePresentation::default(),
            source_capabilities: None,
            layout: LayoutMode::Auto,
            sidebar_visibility: SidebarVisibility::Auto,
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
            experimental: false,
            show_menu_bar: true,
            copy_decorations: false,
            view_preferences_config_path: None,
            prompt_save_view_preferences: true,
            transient_view_preferences: false,
            view_preferences_home_directory: std::env::var_os("HOME").map(PathBuf::from),
            theme: resolve_theme(Some(DEFAULT_DARK_THEME_ID), None, &[]),
            custom_themes: Vec::new(),
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
            external_quit_signal: None,
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
    registered: Arc<RegisteredExtensionPane>,
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
    registration_identity: u64,
    title: String,
    view: Arc<RegisteredFileView>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FileViewMatchCacheKey {
    file_id: String,
    content_identity: String,
    registration_identity: u64,
}

static NEXT_FILE_VIEW_REGISTRATION_IDENTITY: AtomicU64 = AtomicU64::new(1);
const MAX_FILE_VIEW_MODE_TRANSITION_DEPTH: usize = 32;

#[derive(Debug)]
struct FileViewLayoutDispatch {
    task: FileViewLayoutTask,
    file: DiffFile,
    extension: LoadedExtension,
    view_id: String,
    source_capabilities: Option<workdeck_vcs::VcsSourceCapabilities>,
}

fn spawn_file_view_layout_dispatch(
    dispatch: FileViewLayoutDispatch,
    sender: std::sync::mpsc::Sender<FileViewLayoutWorkerResult>,
) {
    std::thread::spawn(move || {
        let FileViewLayoutDispatch {
            task,
            file,
            mut extension,
            view_id,
            source_capabilities,
        } = dispatch;
        let snapshot = create_file_view_input_snapshot(&file);
        let outcome = if task.cancellation.is_cancelled() {
            FileViewLayoutOutcome::Retry
        } else {
            match extension.file_view_matches(&view_id, snapshot.file.as_ref().clone()) {
                Ok(false) => FileViewLayoutOutcome::Declined,
                Err(HostError::Busy(_)) => FileViewLayoutOutcome::Retry,
                Err(_) => FileViewLayoutOutcome::Failed {
                    category: "matches".into(),
                    warning: format!(
                        "Extension {} file view \"{}\" failed matching {} • using raw diff",
                        task.identity.extension_id, task.identity.view_id, task.identity.file_path
                    ),
                },
                Ok(true) => 'layout: {
                    let loaded = source_capabilities
                        .as_ref()
                        .map(|capabilities| capabilities.with_source_snapshots(&file))
                        .transpose();
                    let file = match loaded {
                        Ok(Some(file)) => file,
                        Ok(None) => file,
                        Err(error) => {
                            break 'layout FileViewLayoutOutcome::Failed {
                                category: "unavailable-source".into(),
                                warning: format!(
                                    "File view source unavailable: {error} • using raw diff"
                                ),
                            };
                        }
                    };
                    let input = create_file_view_input(
                        &file,
                        task.identity.width,
                        task.cancellation.clone(),
                        Some(&snapshot),
                    );
                    match extension.layout_file_view_with_timeout(
                        &view_id,
                        input,
                        FILE_VIEW_LAYOUT_TIMEOUT,
                    ) {
                        Ok(Some(layout)) => FileViewLayoutOutcome::Prepared(layout),
                        Ok(None) => FileViewLayoutOutcome::Declined,
                        Err(HostError::Busy(_)) => FileViewLayoutOutcome::Retry,
                        Err(error) => {
                            let detail = error.to_string();
                            let view = format!(
                                "Extension {} file view \"{}\"",
                                task.identity.extension_id, task.identity.view_id
                            );
                            if let Some(issue) = detail.split("invalid layout: ").nth(1) {
                                FileViewLayoutOutcome::Failed {
                                    category: format!("invalid layout: {issue}"),
                                    warning: format!(
                                        "{view} returned an invalid layout: {issue} • using raw diff"
                                    ),
                                }
                            } else if detail.contains("unavailable source: ") {
                                FileViewLayoutOutcome::Failed {
                                    category: "unavailable-source".into(),
                                    warning: format!(
                                        "{view} needs source for {} that Workdeck could not read • using raw diff",
                                        task.identity.file_path
                                    ),
                                }
                            } else {
                                FileViewLayoutOutcome::Failed {
                                    category: "layout".into(),
                                    warning: format!(
                                        "{view} failed laying out {} • using raw diff",
                                        task.identity.file_path
                                    ),
                                }
                            }
                        }
                    }
                }
            }
        };
        task.cancellation.cancel();
        let _ = sender.send(FileViewLayoutWorkerResult {
            request_id: task.request_id,
            outcome,
        });
    });
}

fn requested_file_view_key(extension_id: &str, view_id: &str) -> String {
    if view_id.contains(':') {
        view_id.to_owned()
    } else {
        format!("{extension_id}:{view_id}")
    }
}

fn resolve_live_file_view(
    registrations: &[LiveFileViewRegistration],
    extension_id: &str,
    view_id: &str,
) -> Option<LiveFileViewRegistration> {
    let key = requested_file_view_key(extension_id, view_id);
    registrations
        .iter()
        .find(|registration| registered_file_view_key(&registration.view) == key)
        .cloned()
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
    activation_id: u64,
    extension_index: usize,
    extension_id: String,
    view_id: String,
    view_key: String,
    registration_identity: u64,
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
    navigation: ExtensionRuntimeNavigation,
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
    selection: workdeck_extension_api::ExtensionReviewSelection,
    open_panes: Vec<String>,
    active_keyboard_mode: Option<String>,
    cwd: PathBuf,
    review: workdeck_extension_api::ExtensionReviewSnapshot,
    commands: ExtensionCommandAvailability,
    workspace: Option<workdeck_extension_api::ExtensionWorkspaceSnapshot>,
    file_views: ExtensionFileViewContext,
}

#[derive(Debug, Clone)]
enum QueuedExtensionRequest {
    Command(Box<QueuedExtensionCommand>),
    Event(Box<QueuedExtensionEvent>),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FocusedExtensionPaneInput {
    pane_key: String,
    registration_identity: u64,
    extension_index: usize,
    extension_id: String,
    pane_id: String,
    input_id: String,
    value: String,
    cursor: usize,
    bounds: Rect,
    prefix_cells: u16,
}

#[derive(Debug, Clone)]
struct FocusedExtensionPaneInputCandidate {
    pane_key: String,
    registration_identity: u64,
    extension_index: usize,
    extension_id: String,
    pane_id: String,
    input_id: String,
    value: String,
    bounds: Rect,
    prefix_cells: u16,
}

#[derive(Debug, Clone)]
struct FlattenedPaneInput {
    input_id: String,
    value: String,
    focused: bool,
    prefix_cells: u16,
}

#[derive(Debug, Clone)]
struct CachedPaneRender {
    signature: PaneRenderSignature,
    view: ExtensionPaneView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneRenderSignature {
    registration_identity: u64,
    generation: u64,
    selection: ReviewSelection,
    files: Vec<workdeck_extension_api::ExtensionDiffFile>,
    selected_file_id: Option<String>,
    selected_hunk_index: Option<usize>,
    current_line: Option<ExtensionCurrentLinePaintContext>,
    keybindings: ExtensionResolvedKeybindings,
    placement: PanePlacement,
    width: u16,
    height: u16,
    theme: ExtensionPaintTheme,
}

#[derive(Debug, Clone)]
struct CachedPaneAvailability {
    signature: PaneAvailabilitySignature,
    available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneAvailabilitySignature {
    registration_identity: u64,
    files: Vec<workdeck_extension_api::ExtensionDiffFile>,
    selected_file_id: Option<String>,
    selected_hunk_index: Option<usize>,
    current_line: Option<ExtensionCurrentLinePaintContext>,
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
    file_view_match_cache: BTreeMap<FileViewMatchCacheKey, bool>,
    file_view_epochs: workdeck_extension_host::ScopedEpochState,
    file_view_layouts: FileViewLayoutController,
    file_view_component_expanded: BTreeSet<FileViewComponentStateKey>,
    file_view_component_hits: Vec<FileViewComponentHit>,
    file_view_component_pointer: MouseCapture<FileViewComponentPointer>,
    active_file_view_mode: Option<ActiveFileViewModeRuntime>,
    next_file_view_mode_activation_id: u64,
    active_keyboard_mode: Option<ActiveKeyboardMode>,
    keyboard_mode_controller: KeyboardModeControllerState,
    dialogs: ExtensionDialogQueue,
    pane_action_hits: Vec<ExtensionPaneActionHit>,
    focused_pane_input: Option<FocusedExtensionPaneInput>,
    open: BTreeSet<String>,
    failed_pane_registration_ids: BTreeSet<u64>,
    cached_availability: BTreeMap<u64, CachedPaneAvailability>,
    force_builtin_files_sidebar: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerticalScrollbarRenderMap {
    track: Rect,
    thumb: Rect,
    geometry: VerticalScrollbarGeometry,
    scroll_top: usize,
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
                        title,
                        interactive_mode,
                        ..
                    } => file_views.push(LiveFileViewRegistration {
                        extension_index,
                        registration_identity: NEXT_FILE_VIEW_REGISTRATION_IDENTITY
                            .fetch_add(1, Ordering::Relaxed),
                        title: sanitize_terminal_line(title),
                        view: Arc::new(RegisteredFileView {
                            extension_id: extension.manifest.id.clone(),
                            view_id: id.clone(),
                            interactive_mode: *interactive_mode,
                        }),
                    }),
                    Registration::LineHighlighter { id } => {
                        line_highlighters.push(RegisteredLineHighlighter::new(
                            extension_index,
                            extension.manifest.id.clone(),
                            id.clone(),
                        ));
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
                registered,
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

    fn apply_to_changeset_with_sources(
        &mut self,
        mut changeset: Changeset,
        mut source_capabilities: Option<&mut workdeck_vcs::VcsSourceCapabilities>,
    ) -> Changeset {
        let mut language_registry = LanguageRegistry::default();
        language_registry.replace_extensions(self.file_languages.clone());
        for file in &mut changeset.files {
            let language = language_registry.language_for_path(&file.path);
            let language = (language != "text").then_some(language);
            if file.language != language {
                let original = file.clone();
                file.language = language;
                file.refresh_identity();
                if let Some(capabilities) = &mut source_capabilities {
                    capabilities.rebind_file(&original, file);
                }
            }
        }
        for extension in &mut self.extensions {
            changeset = extension.apply_changeset_transforms_with_sources(
                changeset,
                source_capabilities.as_deref_mut(),
            );
        }
        changeset.refresh_review_identities();
        changeset
    }

    fn reconcile_panes_from(&mut self, previous: &Self, files_pane_open: bool) -> bool {
        let mut previous_open = previous.open.iter().cloned().collect::<Vec<_>>();
        if files_pane_open || previous.force_builtin_files_sidebar {
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

    fn contain_pane_render_failure(
        &mut self,
        registration: &LivePaneRegistration,
        error: impl std::fmt::Display,
    ) -> Option<ExtensionPaneRenderFailure> {
        if !self
            .failed_pane_registration_ids
            .insert(registration.registered.identity)
        {
            return None;
        }
        self.open.remove(&registration.key);
        self.cached_renders.remove(&registration.key);
        if registration.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY) {
            self.force_builtin_files_sidebar = true;
        }
        Some(extension_pane_render_failure(
            &registration.registered,
            error,
            true,
        ))
    }

    fn contain_pane_availability_failure(
        &mut self,
        registration: &LivePaneRegistration,
        error: impl std::fmt::Display,
    ) -> Option<String> {
        if !self
            .failed_pane_registration_ids
            .insert(registration.registered.identity)
        {
            return None;
        }
        self.open.remove(&registration.key);
        self.cached_renders.remove(&registration.key);
        self.cached_availability
            .remove(&registration.registered.identity);
        if registration.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY) {
            self.force_builtin_files_sidebar = true;
        }
        Some(format!(
            "Extension {} pane \"{}\" availability failed • {error}",
            registration.extension_id, registration.pane.id
        ))
    }

    fn cached_file_view_matches(
        &mut self,
        registration: &LiveFileViewRegistration,
        file: &DiffFile,
    ) -> bool {
        let key = FileViewMatchCacheKey {
            file_id: file.runtime_id.clone(),
            content_identity: file.content_identity.clone(),
            registration_identity: registration.registration_identity,
        };
        if let Some(matches) = self.file_view_match_cache.get(&key) {
            return *matches;
        }
        if registration.extension_index >= self.extensions.len() {
            return false;
        }
        if self.extensions[registration.extension_index].request_pending() {
            return false;
        }
        let matches = contain_file_view_match(
            self.extensions[registration.extension_index].file_view_matches(
                &registration.view.view_id,
                project_extension_diff_file(file),
            ),
        );
        self.file_view_match_cache.insert(key, matches);
        matches
    }

    fn retire_extensions(&mut self) {
        self.line_highlight_preparation.retire();
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

/// Own a newly discovered registry until the AppHost commit gate either adopts
/// it or retires every native process on rollback.
struct ProvisionalExtensionPaneRuntime(Option<ExtensionPaneRuntime>);

impl ProvisionalExtensionPaneRuntime {
    fn new(runtime: ExtensionPaneRuntime) -> Self {
        Self(Some(runtime))
    }

    fn runtime_mut(&mut self) -> &mut ExtensionPaneRuntime {
        self.0
            .as_mut()
            .expect("provisional extension runtime has not been adopted")
    }

    fn adopt(&mut self) -> ExtensionPaneRuntime {
        self.0
            .take()
            .expect("provisional extension runtime is adopted exactly once")
    }
}

impl Drop for ProvisionalExtensionPaneRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = &mut self.0 {
            runtime.retire_extensions();
        }
    }
}

#[derive(Debug)]
pub struct ReviewApp {
    source_requests: workdeck_review::ReviewSourceRequests,
    pending_source_reveal: Option<source_controller::PendingSourceReveal>,
    source_loaders: BTreeMap<String, source_controller::SourceLoaderBinding>,
    deferred_file_view_keys: std::collections::VecDeque<(u64, KeyEvent)>,
    replaying_file_view_key: bool,
    state: Arc<Mutex<ReviewState>>,
    review_producer: workdeck_review::ReviewProducer,
    session_broker_client: Option<workdeck_session::WorkdeckSessionBrokerClient>,
    options: ReviewOptions,
    focus: Focus,
    scroll: usize,
    show_agent_skill: bool,
    agent_skill_dialog_hits: Cell<Option<AgentSkillDialogHits>>,
    clipboard_copy_supported: bool,
    clipboard_copy_request: Option<String>,
    show_help: bool,
    help_scroll: usize,
    help_dialog_hits: Cell<Option<HelpDialogHits>>,
    should_quit: bool,
    interactive_authority_retired: bool,
    view_preference_quit: ViewPreferenceQuitController,
    view_preference_prompt_hits: Mutex<Option<ConfirmDialogRenderMap>>,
    view_preference_prompt_hovered_action_key: Option<String>,
    reload_requested: bool,
    extension_command_epoch: u64,
    editor_requested: bool,
    resolved_command_keys: ResolvedKeymap,
    show_menu_bar: bool,
    copy_decorations: bool,
    status: Option<String>,
    note_composer: Option<ReviewNoteComposer>,
    note_composer_bounds: Cell<Option<Rect>>,
    note_composer_actions: Mutex<Vec<(Rect, AgentInlineNoteAction)>>,
    note_hover_state: DiffSectionBodyState,
    note_hover_epoch: Instant,
    note_hover: Option<(usize, ReviewNoteTarget)>,
    note_hover_hit: Cell<Option<(Rect, ReviewNoteTarget)>>,
    saved_note_hover: Option<String>,
    saved_note_actions: Mutex<Vec<(Rect, AgentInlineNoteAction)>>,
    note_sequence: u64,
    filter: String,
    filter_cursor: usize,
    filter_scroll: Cell<usize>,
    review_width: Cell<u16>,
    review_height: Cell<u16>,
    review_bounds: Cell<Option<Rect>>,
    review_geometry_published: Cell<bool>,
    review_prefetch: Mutex<highlight_prefetch::RapidScrollPrefetch>,
    review_plain_height: Mutex<Option<PlainReviewHeight>>,
    review_scrollbar: Mutex<VerticalScrollbarController>,
    review_scrollbar_hits: Cell<Option<VerticalScrollbarRenderMap>>,
    sidebar_bounds: Cell<Option<Rect>>,
    sidebar_scroll_top: Cell<usize>,
    sidebar_reveal_key: Mutex<Option<SidebarRevealKey>>,
    sidebar_file_hits: Mutex<Vec<SidebarFileHit>>,
    review_file_header_hits: Mutex<Vec<SidebarFileHit>>,
    review_gap_hits: Mutex<Vec<ReviewGapMouseHit>>,
    current_line_row: usize,
    expanded_gaps: BTreeSet<(String, usize)>,
    gap_cursor_restore: BTreeMap<(String, usize), GapCursorRestorePoint>,
    agent_line_highlights: LineHighlightMap,
    highlights: Mutex<HighlightedDiffRuntime>,
    themes: ThemeController,
    theme_selector_dialog_hits: Mutex<Option<ThemeSelectorDialogPlan>>,
    pending_theme_hover_preview: Option<PendingThemeHoverPreview>,
    theme_selector_hovered_item_id: Option<String>,
    startup_notices: StartupNoticeQueue,
    extension_toasts: Arc<Mutex<ExtensionNotificationSurface>>,
    extension_notification_subscription: Option<ExtensionNotificationSubscription>,
    mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration,
    mouse_scroll_accumulator: f64,
    copy_selection_drag: Option<CopySelectionDrag>,
    copy_selection_snapshot: Option<LiveCopySelectionSnapshot>,
    last_copy_click_time: Option<Instant>,
    last_copy_click_point: Option<CopySelectionPoint>,
    copy_click_count: u8,
    extension_pane_runtime: Mutex<ExtensionPaneRuntime>,
    file_presentation_rendering: Mutex<FilePresentationRenderingController>,
    extension_runtime_bridge: ExtensionRuntimeBridge,
    extension_file_projection_cache: Mutex<ExtensionFileProjectionCache>,
    extension_event_context_provider: ExtensionEventContextProviderSlot,
    extension_event_context_installation: Option<ExtensionEventContextProviderInstallation>,
    extension_event_dispatch_depth: usize,
    file_view_mode_transition_depth: usize,
    extension_review_events: ExtensionReviewEventController,
    extension_registry_generation: u64,
    review_projection_generation: u64,
    extension_confirm_dialog_hits: Mutex<Option<ConfirmDialogRenderMap>>,
    extension_confirm_hovered_action_key: Option<String>,
    extension_input_dialog_hits: Mutex<Option<ExtensionInputDialogPlan>>,
    extension_select_dialog_hits: Mutex<Option<ExtensionSelectDialogPlan>>,
    extension_dialog_hovered_action_key: Option<String>,
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
        options: ReviewOptions,
        extensions: Vec<LoadedExtension>,
    ) -> Self {
        let review_producer = workdeck_review::ReviewProducer::from_files(
            &changeset.files,
            Some(changeset.effective_source_label()),
            workdeck_review::ReviewProducerOptions {
                source_loader: source_controller::publication_source_loader(
                    options.source_capabilities.clone(),
                ),
                ..Default::default()
            },
        )
        .expect("the native review producer uses an internally valid generation identity");
        Self::new_with_extensions_and_session(changeset, options, extensions, review_producer, None)
    }

    fn new_with_extensions_and_session(
        changeset: Changeset,
        mut options: ReviewOptions,
        mut extensions: Vec<LoadedExtension>,
        review_producer: workdeck_review::ReviewProducer,
        session_broker_client: Option<workdeck_session::WorkdeckSessionBrokerClient>,
    ) -> Self {
        if options.pager {
            options.sidebar = false;
            options.sidebar_visibility = SidebarVisibility::Hidden;
            options.show_menu_bar = false;
        }
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
        let themes = ThemeController::from_resolved(
            options.theme.id.clone(),
            None,
            options.custom_themes.clone(),
            options.transparent_background,
        );
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
        let initial_files_key = resolve_pane_slot_key(
            &extension_pane_runtime.session_panes,
            WORKDECK_FILES_PANE_KEY,
            &extension_pane_runtime.open,
            &BTreeSet::new(),
        );
        if !extension_pane_runtime
            .session_panes
            .iter()
            .find(|pane| pane.key == initial_files_key)
            .is_some_and(|pane| pane.default_open)
        {
            options.sidebar = false;
        }
        if !options.sidebar {
            let logical_open = extension_pane_runtime.open.clone();
            let files_key = resolve_pane_slot_key(
                &extension_pane_runtime.session_panes,
                WORKDECK_FILES_PANE_KEY,
                &logical_open,
                &BTreeSet::new(),
            );
            extension_pane_runtime.open.remove(&files_key);
        }
        let initial_view_preferences = PersistedViewPreferences {
            mode: match options.layout {
                LayoutMode::Auto => InputLayoutMode::Auto,
                LayoutMode::Split => InputLayoutMode::Split,
                LayoutMode::Stack => InputLayoutMode::Stack,
            },
            theme: Some(themes.committed.clone()),
            show_line_numbers: options.line_numbers,
            wrap_lines: options.wrap_lines,
            show_hunk_headers: options.hunk_headers,
            show_menu_bar: options.show_menu_bar,
            show_agent_notes: options.agent_notes,
            copy_decorations: options.copy_decorations,
            cursor_line: match options.cursor_line {
                CursorLineMode::Row => InputCursorLine::Row,
                CursorLineMode::Number => InputCursorLine::Number,
                CursorLineMode::Off => InputCursorLine::Off,
            },
        };
        let view_preference_quit = ViewPreferenceQuitController::new(
            initial_view_preferences,
            options.view_preferences_config_path.clone(),
            options.pager,
            options.prompt_save_view_preferences,
            options.transient_view_preferences,
            options.view_preferences_home_directory.clone(),
        );
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
        let initial_snapshot = extension_runtime_bridge::SharedRuntimeSnapshot {
            generation: state.generation(),
            changeset: state.changeset_snapshot(),
            selection: state.selection(),
        };
        let mut extension_file_projection_cache = ExtensionFileProjectionCache::default();
        let initial_files =
            extension_file_projection_cache.get(Arc::clone(&initial_snapshot.changeset));
        let initial_selected_file_id = initial_snapshot
            .changeset
            .files
            .get(initial_snapshot.selection.file_index)
            .map(|file| file.runtime_id.clone());
        let mut initial_extension_selection =
            workdeck_extension_host::build_extension_review_selection_from_document(
                &initial_snapshot.changeset,
                initial_snapshot.selection,
            );
        if options.cursor_line == CursorLineMode::Off {
            initial_extension_selection.current_line = None;
        }
        let extension_runtime_bridge = ExtensionRuntimeBridge::new(ExtensionRuntimeCommit {
            registry_generation: 1,
            review_generation: 1,
            review: build_extension_review_snapshot(&state),
            selection: initial_extension_selection,
            snapshot: initial_snapshot,
            files: initial_files,
            selected_file_id: initial_selected_file_id,
            commands: ExtensionCommandAvailability::default(),
        });
        let mut app = Self {
            deferred_file_view_keys: std::collections::VecDeque::new(),
            replaying_file_view_key: false,
            state: Arc::new(Mutex::new(state)),
            review_producer,
            session_broker_client,
            options,
            focus: Focus::Review,
            scroll: 0,
            show_agent_skill: false,
            agent_skill_dialog_hits: Cell::new(None),
            clipboard_copy_supported: false,
            clipboard_copy_request: None,
            show_help: false,
            help_scroll: 0,
            help_dialog_hits: Cell::new(None),
            should_quit: false,
            interactive_authority_retired: false,
            view_preference_quit,
            view_preference_prompt_hits: Mutex::new(None),
            view_preference_prompt_hovered_action_key: None,
            reload_requested: false,
            extension_command_epoch: 1,
            editor_requested: false,
            resolved_command_keys,
            show_menu_bar,
            copy_decorations,
            status: keymap_status,
            note_composer: None,
            note_composer_bounds: Cell::new(None),
            note_composer_actions: Mutex::new(Vec::new()),
            note_hover_state: DiffSectionBodyState::default(),
            note_hover_epoch: Instant::now(),
            note_hover: None,
            note_hover_hit: Cell::new(None),
            saved_note_hover: None,
            saved_note_actions: Mutex::new(Vec::new()),
            note_sequence: 0,
            filter: String::new(),
            filter_cursor: 0,
            filter_scroll: Cell::new(0),
            review_width: Cell::new(120),
            review_height: Cell::new(20),
            review_bounds: Cell::new(None),
            review_geometry_published: Cell::new(false),
            review_prefetch: Mutex::new(highlight_prefetch::RapidScrollPrefetch::default()),
            review_plain_height: Mutex::new(None),
            review_scrollbar: Mutex::new(VerticalScrollbarController::default()),
            review_scrollbar_hits: Cell::new(None),
            sidebar_bounds: Cell::new(None),
            sidebar_scroll_top: Cell::new(0),
            sidebar_reveal_key: Mutex::new(None),
            sidebar_file_hits: Mutex::new(Vec::new()),
            review_file_header_hits: Mutex::new(Vec::new()),
            review_gap_hits: Mutex::new(Vec::new()),
            current_line_row: 0,
            expanded_gaps: BTreeSet::new(),
            source_requests: workdeck_review::ReviewSourceRequests::default(),
            pending_source_reveal: None,
            source_loaders: BTreeMap::new(),
            gap_cursor_restore: BTreeMap::new(),
            agent_line_highlights: LineHighlightMap::default(),
            highlights: Mutex::new(HighlightedDiffRuntime::default()),
            themes,
            theme_selector_dialog_hits: Mutex::new(None),
            pending_theme_hover_preview: None,
            theme_selector_hovered_item_id: None,
            startup_notices,
            extension_toasts,
            extension_notification_subscription,
            mouse_scroll_acceleration: ReviewMouseWheelScrollAcceleration::default(),
            mouse_scroll_accumulator: 0.0,
            copy_selection_drag: None,
            copy_selection_snapshot: None,
            last_copy_click_time: None,
            last_copy_click_point: None,
            copy_click_count: 0,
            extension_pane_runtime: Mutex::new(extension_pane_runtime),
            file_presentation_rendering: Mutex::new(FilePresentationRenderingController::default()),
            extension_runtime_bridge,
            extension_file_projection_cache: Mutex::new(extension_file_projection_cache),
            extension_event_context_provider,
            extension_event_context_installation: None,
            extension_event_dispatch_depth: 0,
            file_view_mode_transition_depth: 0,
            extension_review_events: ExtensionReviewEventController::default(),
            extension_registry_generation: 1,
            review_projection_generation: 1,
            extension_confirm_dialog_hits: Mutex::new(None),
            extension_confirm_hovered_action_key: None,
            extension_input_dialog_hits: Mutex::new(None),
            extension_select_dialog_hits: Mutex::new(None),
            extension_dialog_hovered_action_key: None,
            #[cfg(test)]
            observed_extension_events: Vec::new(),
            extension_trust_controller,
            extension_trust_request: None,
            extension_trust_prompt_hits: Cell::new(None),
        };
        if let Some(capabilities) = app.options.source_capabilities.take() {
            app.install_vcs_source_capabilities(&capabilities);
        }
        app.seed_current_line_cursor();
        app.commit_extension_runtime_bridge();
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

    #[must_use]
    pub fn review_producer(&self) -> workdeck_review::ReviewProducer {
        self.review_producer.clone()
    }

    #[must_use]
    pub fn session_broker_client(&self) -> Option<workdeck_session::WorkdeckSessionBrokerClient> {
        self.session_broker_client.clone()
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

    /// Resolve one file's highlighting synchronously into this app's rendering cache.
    /// Embedders can separate syntax preparation from subsequent interaction measurements.
    /// Does not navigate, render, or create repository state.
    pub fn prefetch_file_highlights(&self, file_index: usize) -> bool {
        if !self.options.highlight {
            return false;
        }
        let Some(file) = self.with_state(|state| state.changeset().files.get(file_index).cloned())
        else {
            return false;
        };
        self.highlights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .prefetch_highlighted_diff_shared(&file, &self.options.theme, false)
            .is_some()
    }

    /// Consume a reload requested by a host-mediated extension write.
    pub fn take_reload_requested(&mut self) -> bool {
        std::mem::take(&mut self.reload_requested)
    }

    pub fn take_quit_requested(&mut self) -> bool {
        std::mem::take(&mut self.should_quit)
    }

    /// Cross the same extension-retirement boundary as the interactive host when its external
    /// signal fires. This is explicit so alternate terminal hosts and deterministic tests cannot
    /// observe quit before subscribed extensions receive their final shutdown notification.
    pub fn process_external_quit_signal(&mut self) -> bool {
        let requested = self
            .options
            .external_quit_signal
            .as_ref()
            .is_some_and(|signal| signal.load(Ordering::Acquire));
        if !requested {
            return false;
        }
        self.retire_interactive_authority();
        self.should_quit = true;
        true
    }

    /// Whether the host has crossed its terminal shutdown boundary.
    ///
    /// Signal ownership stays outside the renderer, but every irreversible or
    /// publication-changing operation consults the same projected authority.
    fn shutdown_requested(&self) -> bool {
        self.should_quit
            || self.interactive_authority_retired
            || self
                .options
                .external_quit_signal
                .as_ref()
                .is_some_and(|signal| signal.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn save_config_prompt_open(&self) -> bool {
        self.view_preference_quit.save_config_prompt_open()
    }

    #[must_use]
    pub fn changed_view_preferences(&self) -> Vec<workdeck_core::ViewPreferenceChange> {
        self.view_preference_quit
            .changed_view_preferences(&self.current_view_preferences())
    }

    fn current_view_preferences(&self) -> PersistedViewPreferences {
        PersistedViewPreferences {
            mode: match self.layout() {
                LayoutMode::Auto => InputLayoutMode::Auto,
                LayoutMode::Split => InputLayoutMode::Split,
                LayoutMode::Stack => InputLayoutMode::Stack,
            },
            theme: Some(self.themes.committed.clone()),
            show_line_numbers: self.options.line_numbers,
            wrap_lines: self.options.wrap_lines,
            show_hunk_headers: self.options.hunk_headers,
            show_menu_bar: self.show_menu_bar,
            show_agent_notes: self.options.agent_notes,
            copy_decorations: self.copy_decorations,
            cursor_line: match self.options.cursor_line {
                CursorLineMode::Row => InputCursorLine::Row,
                CursorLineMode::Number => InputCursorLine::Number,
                CursorLineMode::Off => InputCursorLine::Off,
            },
        }
    }

    /// Rebuild the descriptor registered by Hunk's current-review hook from
    /// the values that are live now, rather than from launch-time options.
    fn current_review_reload_request(&self) -> Option<WorkspaceRefreshRequest> {
        let input = self.options.review_input.as_ref()?;
        let source_label =
            self.with_state(|state| state.changeset().effective_source_label().to_owned());
        derive_workspace_refresh_request(
            input,
            &source_label,
            &CurrentReviewViewOptions {
                layout_mode: match self.layout() {
                    LayoutMode::Auto => InputLayoutMode::Auto,
                    LayoutMode::Split => InputLayoutMode::Split,
                    LayoutMode::Stack => InputLayoutMode::Stack,
                },
                theme_id: self.themes.committed.clone(),
                show_agent_notes: self.options.agent_notes,
                show_hunk_headers: self.options.hunk_headers,
                show_line_numbers: self.options.line_numbers,
                show_menu_bar: self.show_menu_bar,
                wrap_lines: self.options.wrap_lines,
            },
        )
    }

    fn request_quit(&mut self) {
        let current = self.current_view_preferences();
        match self.view_preference_quit.request_quit(&current) {
            QuitRequestOutcome::Locked => {}
            QuitRequestOutcome::PromptOpened => {
                self.show_help = false;
                self.help_dialog_hits.set(None);
                self.view_preference_prompt_hovered_action_key = None;
            }
            QuitRequestOutcome::QuitNow => self.should_quit = true,
        }
    }

    fn save_view_preferences_and_quit(&mut self, now: Instant) {
        let current = self.current_view_preferences();
        match self
            .view_preference_quit
            .save_view_preferences_and_schedule_quit(&current, now)
        {
            Ok(Some(path)) => {
                self.status = Some(format!("Saved view preferences to {}", path.display()));
                self.clear_view_preference_prompt_render_state();
            }
            Ok(None) => {}
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn discard_view_preferences_and_quit(&mut self) {
        if self
            .view_preference_quit
            .discard_view_preferences_and_quit()
        {
            self.clear_view_preference_prompt_render_state();
            self.should_quit = true;
        }
    }

    fn never_ask_to_save_view_preferences_and_quit(&mut self, now: Instant) {
        match self.view_preference_quit.never_ask_and_schedule_quit(now) {
            Ok(Some(path)) => {
                self.status = Some(format!(
                    "Won't ask to save view preferences again ({})",
                    path.display()
                ));
                self.clear_view_preference_prompt_render_state();
            }
            Ok(None) => {}
            Err(error) => self.status = Some(error.to_string()),
        }
    }

    fn close_save_config_prompt(&mut self) {
        self.view_preference_quit.close_save_config_prompt();
        if !self.view_preference_quit.save_config_prompt_open() {
            self.clear_view_preference_prompt_render_state();
        }
    }

    fn clear_view_preference_prompt_render_state(&mut self) {
        *self
            .view_preference_prompt_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        self.view_preference_prompt_hovered_action_key = None;
    }

    fn handle_save_config_prompt_key(&mut self, key: &KeyEvent) -> bool {
        if !self.view_preference_quit.save_config_prompt_open() {
            return false;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char('s') => {
                self.save_view_preferences_and_quit(Instant::now());
            }
            KeyCode::Char('q') => self.discard_view_preferences_and_quit(),
            KeyCode::Char('n') => {
                self.never_ask_to_save_view_preferences_and_quit(Instant::now());
            }
            KeyCode::Esc => self.close_save_config_prompt(),
            _ => {}
        }
        true
    }

    /// Tell the native UI whether its host can service clipboard requests.
    pub fn set_clipboard_copy_supported(&mut self, supported: bool) {
        self.clipboard_copy_supported = supported;
    }

    /// Consume one host-owned clipboard write requested by a review surface.
    pub fn take_clipboard_copy_request(&mut self) -> Option<String> {
        self.clipboard_copy_request.take()
    }

    /// Replace the optimistic source-compatible notice when the host copy fails.
    pub fn report_clipboard_copy_failure(&mut self, error: impl std::fmt::Display) {
        self.status = Some(format!("Clipboard copy failed: {error}"));
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

    /// Persist one queued security decision and tell AppHost whether it must
    /// enqueue an extension-aware current-review refresh.
    #[must_use]
    pub fn process_extension_trust_request(&mut self, can_reload_extensions: bool) -> bool {
        let Some(request) = self.take_extension_trust_request() else {
            return false;
        };
        let Some(handler) = self.options.extension_trust_handler.clone() else {
            self.status = Some("Failed to record the trust decision.".into());
            return false;
        };
        match handler.run(&request.repo_root, request.decision) {
            Ok(()) => {}
            Err(ExtensionTrustHostError::Write(error)) => {
                self.status = Some(error.notice());
                return false;
            }
        }
        self.reconcile_extension_trust_repo_root(None);
        if request.decision == workdeck_extension_host::TrustDecision::Denied {
            self.status = Some("Won't run this repository's extensions".into());
            return false;
        }
        if !can_reload_extensions {
            self.status =
                Some("Trusted this repository • restart Workdeck to load its extensions".into());
            return false;
        }
        true
    }

    #[must_use]
    pub fn take_editor_request(&mut self) -> Option<ReviewEditorRequest> {
        if !std::mem::take(&mut self.editor_requested) {
            return None;
        }
        let current_line_cursor = self.current_review_line_cursor();
        let (file, line_cursor, selected_hunk) = self.with_state(|state| {
            let selection = state.selection();
            let cursor_target = current_line_cursor.map(|cursor| cursor.target);
            let file_index = cursor_target.map_or(selection.file_index, |target| target.file_index);
            let file = state.changeset().files.get(file_index).cloned();
            let hunk_index = cursor_target
                .map(|target| target.hunk_index)
                .or(selection.hunk_index);
            let selected_hunk = file
                .as_ref()
                .and_then(|file| hunk_index.and_then(|index| file.hunks.get(index)))
                .cloned();
            let line_cursor = file.as_ref().and_then(|file| {
                let (hunk_index, side, line) = cursor_target
                    .map(|target| (target.hunk_index, target.side, target.line))
                    .or_else(|| Some((selection.hunk_index?, selection.side?, selection.line?)))?;
                Some(EditorLineCursor {
                    file_id: if file.runtime_id.is_empty() {
                        file.key.clone()
                    } else {
                        file.runtime_id.clone()
                    },
                    hunk_index,
                    target: EditorLineTarget { side, line },
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

    fn prepare_reloaded_changeset(&self, changeset: Changeset) -> Changeset {
        self.prepare_reloaded_changeset_with_sources(changeset, None)
    }

    fn prepare_reloaded_changeset_with_sources(
        &self,
        changeset: Changeset,
        source_capabilities: Option<&mut workdeck_vcs::VcsSourceCapabilities>,
    ) -> Changeset {
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .apply_to_changeset_with_sources(changeset, source_capabilities)
    }

    fn reload_with_reason(
        &mut self,
        changeset: Changeset,
        reason: SessionReloadReason,
        emit_startup: bool,
    ) {
        let changeset = self.prepare_reloaded_changeset(changeset);
        if self.with_state(|state| state.changeset() != &changeset)
            && let Err(error) = self.review_producer.publish_with_source_loader(
                &workdeck_review::PublishReviewInput {
                    files: changeset.files.clone(),
                    source_label: Some(changeset.effective_source_label().to_owned()),
                },
                source_controller::publication_source_loader(None),
            )
        {
            self.status = Some(format!("review publication failed: {error}"));
            return;
        }
        self.commit_reloaded_changeset(changeset, reason, emit_startup, false);
    }

    /// Commit a changeset whose extension transforms and producer publication
    /// have already succeeded. No fallible host operation belongs below this
    /// boundary, matching AppHost's broker/publication commit gate.
    fn commit_reloaded_changeset(
        &mut self,
        changeset: Changeset,
        reason: SessionReloadReason,
        emit_startup: bool,
        reset_app: bool,
    ) {
        if reset_app {
            self.pending_source_reveal = None;
            self.source_requests
                .retire(&self.source_loaders.keys().cloned().collect());
            self.source_loaders.clear();
            self.options.source_capabilities = None;
            self.options.source_presentation = ReviewSourcePresentation::default();
        } else {
            self.reconcile_source_loaders(&changeset);
        }
        self.cancel_copy_selection();
        self.reset_copy_click_sequence();
        self.extension_command_epoch = self.extension_command_epoch.saturating_add(1);
        self.review_projection_generation = self.review_projection_generation.saturating_add(1);
        // Revoke retained review controls synchronously, before reload cleanup or lifecycle work.
        self.commit_extension_runtime_bridge();
        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
        {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.pane_action_hits.clear();
            runtime.line_highlight_preparation.replace_document();
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
            runtime.file_view_epochs =
                reconcile_file_view_epochs(&runtime.file_view_epochs, &file_ids, &view_keys);
            runtime.file_view_match_cache.clear();
            runtime.file_view_layouts.clear();
            runtime.file_view_component_expanded.clear();
            runtime.file_view_component_hits.clear();
            runtime.file_view_component_pointer.release();
        }
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .line_highlights
            .reconcile_files(changeset.files.iter().map(|file| file.runtime_id.clone()));
        if self.with_state(|state| state.changeset() != &changeset) {
            let previous_changeset = self.with_state(|state| state.changeset().clone());
            if !self.expanded_gaps.is_empty() || !self.gap_cursor_restore.is_empty() {
                // Hunk MIT: useTerminalReview's document-reconcile effect and shared
                // review reducer retire expansion state by semantic source identity.
                let retired = workdeck_review::review_keys_with_retired_source_identities(
                    previous_changeset
                        .files
                        .iter()
                        .map(|file| (file.key.as_str(), file.source_identity.as_deref())),
                    changeset
                        .files
                        .iter()
                        .map(|file| (file.key.as_str(), file.source_identity.as_deref())),
                );
                self.expanded_gaps
                    .retain(|(file_key, _)| !retired.contains(file_key.as_str()));
                let mut file_indices = BTreeMap::new();
                for (index, file) in changeset.files.iter().enumerate() {
                    file_indices.entry(file.key.as_str()).or_insert(index);
                }
                self.gap_cursor_restore.retain(|(file_key, _), restore| {
                    if retired.contains(file_key.as_str()) {
                        return false;
                    }
                    let Some(index) = file_indices.get(restore.file_key.as_str()) else {
                        return false;
                    };
                    // Source cursors address a file identity, not its position in the
                    // stream. Rebase the native indexed target onto that same file.
                    restore.target.file_index = *index;
                    true
                });
            }
            let carried_agent_line_highlights = carry_over_line_highlights(
                &self.agent_line_highlights,
                &previous_changeset,
                &changeset,
            );
            self.with_state(|state| {
                let text = state.selected_file().and_then(|file| {
                    if state.selection().side != Some(review_expansion_side(file.change_kind)) {
                        return None;
                    }
                    match self.options.source_presentation.status(file) {
                        Some(workdeck_review::ReviewSourceStatus::Loaded { text }) => {
                            Some(text.as_str())
                        }
                        _ => None,
                    }
                });
                state.reload_with_selected_source(changeset, text);
            });
            self.agent_line_highlights = carried_agent_line_highlights;
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
        // Retire dialogs only after the replacement review is authoritative. A
        // cancelled async extension handler may immediately enter a file-view
        // mode, and that mode must belong to the review the reload produced.
        self.reconcile_active_file_view_mode();
        self.cancel_extension_dialogs_for_reload();
        if reset_app {
            self.with_state(|state| {
                if !state.changeset().files.is_empty() {
                    let _ = state.select_file(0);
                }
            });
            self.focus = Focus::Review;
            self.scroll = 0;
            self.current_line_row = 0;
            self.filter.clear();
            self.filter_cursor = 0;
            self.filter_scroll.set(0);
            self.expanded_gaps.clear();
            self.gap_cursor_restore.clear();
            self.options.horizontal_offset = 0;
        }
        self.commit_extension_runtime_bridge();
        let immediate = self.update_extension_review_events(Instant::now());
        self.publish_extension_lifecycle_events(immediate);
        if emit_startup {
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::Startup {
                cwd: self.extension_command_cwd(),
            });
        }
        self.publish_current_changeset_event(true, reason);
    }

    #[cfg(test)]
    fn replace_extensions_and_reload(
        &mut self,
        changeset: Changeset,
        extensions: Vec<LoadedExtension>,
    ) {
        let mut replacement = ProvisionalExtensionPaneRuntime::new(ExtensionPaneRuntime::new(
            extensions,
            &changeset.files,
        ));
        let changeset = replacement
            .runtime_mut()
            .apply_to_changeset_with_sources(changeset, None);
        if self.with_state(|state| state.changeset() != &changeset)
            && let Err(error) = self
                .review_producer
                .publish(&workdeck_review::PublishReviewInput {
                    files: changeset.files.clone(),
                    source_label: Some(changeset.effective_source_label().to_owned()),
                })
        {
            self.status = Some(format!("review publication failed: {error}"));
            return;
        }
        self.install_extension_runtime(replacement.adopt(), &changeset);
        self.commit_reloaded_changeset(changeset, SessionReloadReason::Manual, true, true);
    }

    fn install_extension_runtime(
        &mut self,
        mut replacement: ExtensionPaneRuntime,
        changeset: &Changeset,
    ) {
        self.cancel_extension_dialogs_for_reload();
        self.exit_active_keyboard_mode();
        self.exit_active_file_view_mode();
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
            let file_ids = changeset
                .files
                .iter()
                .map(|file| file.runtime_id.clone())
                .collect::<Vec<_>>();
            let view_keys = replacement
                .file_views
                .iter()
                .map(|registration| registered_file_view_key(&registration.view))
                .collect::<BTreeSet<_>>();
            replacement.file_view_selections = reconcile_file_view_selections(
                &runtime.file_view_selections,
                &file_ids,
                &view_keys,
            );
            replacement
                .line_highlights
                .retain_epochs_from(&runtime.line_highlights);
            std::mem::replace(&mut *runtime, replacement)
        };
        previous.retire_extensions();
        drop(previous);
        self.extension_registry_generation = self.extension_registry_generation.saturating_add(1);
        self.install_extension_event_context_provider();
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = Some(status.into());
    }

    pub fn tick_extension_notifications(&mut self, now: Instant) {
        if self.note_hover_state.advance_time(
            now.saturating_duration_since(self.note_hover_epoch)
                .as_millis() as u64,
        ) == AddNoteAffordanceUpdate::Clear
        {
            self.note_hover = None;
            self.note_hover_hit.set(None);
        }
        if self.view_preference_quit.poll_quit(now) {
            self.should_quit = true;
        }
        self.review_scrollbar
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .tick(now);
        self.tick_theme_hover_preview(now);
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

    /// Cursor requested by a focused, host-rendered extension-pane input.
    #[must_use]
    pub fn extension_pane_input_cursor_position(&self) -> Option<Position> {
        if self
            .note_composer
            .as_ref()
            .is_some_and(|draft| draft.focused)
            || self.view_preference_quit.save_config_prompt_open()
            || self.extension_trust_prompt_root().is_some()
            || self.themes.selector_open
            || self.show_agent_skill
            || self.focus == Focus::Filter
            || self.show_help
        {
            return None;
        }
        let runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if runtime.menu.is_open() || runtime.dialogs.current().is_some() {
            return None;
        }
        let active = runtime.focused_pane_input.clone()?;
        drop(runtime);
        let prefix = active
            .value
            .chars()
            .take(active.cursor)
            .collect::<String>()
            .width();
        let offset = usize::from(active.prefix_cells).saturating_add(prefix);
        let max_offset = usize::from(active.bounds.width.saturating_sub(1));
        Some(Position::new(
            active.bounds.x.saturating_add(
                u16::try_from(offset.min(max_offset)).unwrap_or(active.bounds.width),
            ),
            active.bounds.y,
        ))
    }

    /// Cursor requested by the focused status-bar filter input.
    #[must_use]
    pub fn status_filter_cursor_position(&self, area: Rect) -> Option<Position> {
        if let (Some(composer), Some(bounds)) = (
            self.note_composer.as_ref().filter(|draft| draft.focused),
            self.note_composer_bounds.get(),
        ) {
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
        if !self.replaying_file_view_key
            && let Some((activation, _)) = self.deferred_file_view_keys.back().copied()
        {
            self.deferred_file_view_keys.push_back((activation, key));
            return;
        }
        self.clear_note_hover();
        if self.handle_extension_trust_prompt_key(&key) {
            return;
        }
        if self.handle_save_config_prompt_key(&key) {
            return;
        }
        if self.handle_workspace_write_key(&key)
            || self.handle_extension_confirm_key(&key)
            || self.handle_extension_select_key(&key)
            || self.handle_extension_input_key(&key)
        {
            return;
        }

        // A note editor owns menu-toggle keys without opening chrome over the
        // draft. Other focused editors are intentionally below the toggle,
        // matching Hunk's app-level ownership chain.
        if self
            .note_composer
            .as_ref()
            .is_some_and(|draft| draft.focused)
            && Self::is_app_menu_toggle_key(&key)
        {
            return;
        }
        if self.handle_app_menu_toggle_key(&key) {
            return;
        }

        if self.show_agent_skill && key.code == KeyCode::Esc {
            self.show_agent_skill = false;
            self.agent_skill_dialog_hits.set(None);
            return;
        }
        if self.show_help
            && matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            )
        {
            self.show_help = false;
            self.help_dialog_hits.set(None);
            return;
        }
        if self.handle_theme_selector_key(&key) {
            return;
        }
        if self.handle_app_menu_key(&key) {
            return;
        }
        if self.handle_filter_key(&key) {
            return;
        }
        if self.handle_note_composer_key(&key) {
            return;
        }
        if self.handle_focused_extension_pane_input(&key) {
            return;
        }
        if self.route_active_file_view_mode(&key) {
            return;
        }
        if self.route_active_keyboard_mode(&key) {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.request_quit();
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

    fn handle_theme_selector_key(&mut self, key: &KeyEvent) -> bool {
        if !self.themes.selector_open {
            return false;
        }
        if key.code == KeyCode::Esc {
            self.close_theme_selector();
            return true;
        }
        let live_key = to_live_extension_key_event(key);
        let commands = self.builtin_commands();
        if let Some(direction) = vertical_command_direction(&commands, &live_key) {
            self.move_theme_selector(direction.delta());
            return true;
        }
        match key.code {
            KeyCode::Up => self.move_theme_selector(-1),
            KeyCode::Down => self.move_theme_selector(1),
            KeyCode::BackTab => self.move_theme_selector(-1),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_theme_selector(-1);
            }
            KeyCode::Tab => self.move_theme_selector(1),
            KeyCode::Enter => self.accept_theme_selector(),
            _ => {}
        }
        true
    }

    fn handle_note_composer_key(&mut self, key: &KeyEvent) -> bool {
        if !self
            .note_composer
            .as_ref()
            .is_some_and(|draft| draft.focused)
        {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            self.save_note_composer();
            return true;
        }
        if key.code == KeyCode::Esc {
            self.note_composer = None;
            self.note_composer_bounds.set(None);
            self.focus = Focus::Review;
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
                KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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
        if !composer.focused {
            return;
        }
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
            kind: ReviewNoteComposerKind::Create,
            focused: true,
            thread: None,
            target,
            body: String::new(),
            cursor: 0,
        });
        self.reconcile_active_file_view_mode();
        self.status = None;
        if self.options.cursor_line == CursorLineMode::Off {
            let _ = self
                .with_state(|state| state.reveal_line(target.file_index, target.side, target.line));
            self.scroll_to_selected_line();
        }
    }

    fn active_note_for_composer(
        &self,
        editable_only: bool,
    ) -> Option<(ReviewComment, ReviewNoteTarget)> {
        let current_target = self.current_note_target();
        self.with_state(|state| {
            let selection = state.selection();
            let file = state.changeset().files.get(selection.file_index)?;
            let selected_hunk = selection.hunk_index?;
            state
                .comments()
                .iter()
                .filter(|comment| {
                    comment.resolution == ReviewNoteResolution::Active
                        && comment.anchor.file_key == file.key
                        && (comment.anchor.owner_hunk_index == Some(selected_hunk)
                            || comment
                                .anchor
                                .intersecting_hunk_indices
                                .contains(&selected_hunk))
                        && (self.options.agent_notes || comment.source == "user")
                        && (!editable_only || (comment.source == "user" && comment.editable))
                })
                .min_by_key(|comment| {
                    if self.saved_note_hover.as_deref() == Some(comment.id.as_str()) {
                        return 0;
                    }
                    let side = comment.side.or(comment.anchor.preferred_side);
                    let line = comment.line.or(comment.anchor.preferred_line);
                    1 + usize::from(current_target.is_none_or(|target| {
                        side != Some(target.side) || line != Some(target.line)
                    }))
                })
                .and_then(|comment| {
                    let hunk_index = comment
                        .anchor
                        .owner_hunk_index
                        .or_else(|| comment.anchor.intersecting_hunk_indices.first().copied())
                        .unwrap_or(selected_hunk);
                    Some((
                        comment.clone(),
                        ReviewNoteTarget {
                            file_index: selection.file_index,
                            hunk_index,
                            side: comment.side.or(comment.anchor.preferred_side)?,
                            line: comment.line.or(comment.anchor.preferred_line)?,
                        },
                    ))
                })
        })
    }

    fn open_active_note_edit(&mut self) {
        let Some((note, target)) = self.active_note_for_composer(true) else {
            self.status = Some("no editable user note is active".into());
            return;
        };
        self.note_sequence = self.note_sequence.saturating_add(1);
        let thread = self.with_state(|state| saved_comment_thread(&note, state.comments()));
        let body = note.summary;
        let cursor = 0;
        self.note_composer = Some(ReviewNoteComposer {
            id: format!("user-note-draft-{}", self.note_sequence),
            focused: true,
            kind: ReviewNoteComposerKind::Edit {
                target_note_id: note.id,
                parent_id: note.parent_id,
            },
            thread: Some(thread),
            target,
            body,
            cursor,
        });
        self.reconcile_active_file_view_mode();
        self.focus = Focus::Review;
        self.status = None;
    }

    fn open_active_note_reply(&mut self) {
        let Some((note, target)) = self.active_note_for_composer(false) else {
            self.status = Some("no review note is active".into());
            return;
        };
        self.note_sequence = self.note_sequence.saturating_add(1);
        let thread = self.with_state(|state| {
            let parent = saved_comment_thread(&note, state.comments());
            let mut ancestors = parent.ancestor_has_next_sibling;
            if parent.depth > 0 {
                ancestors.push(parent.has_next_sibling.unwrap_or(false));
            }
            VisibleAgentNoteThread {
                note_id: format!("user-note-{}", self.note_sequence),
                parent_id: Some(note.id.clone()),
                depth: parent.depth + 1,
                has_next_sibling: Some(false),
                ancestor_has_next_sibling: ancestors,
            }
        });
        self.note_composer = Some(ReviewNoteComposer {
            id: format!("user-note-{}", self.note_sequence),
            kind: ReviewNoteComposerKind::Reply { parent_id: note.id },
            focused: true,
            thread: Some(thread),
            target,
            body: String::new(),
            cursor: 0,
        });
        self.reconcile_active_file_view_mode();
        self.focus = Focus::Review;
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
                .map(|file| {
                    let (id, parent_id) = match &composer.kind {
                        ReviewNoteComposerKind::Create => (composer.id.as_str(), None),
                        ReviewNoteComposerKind::Edit {
                            target_note_id,
                            parent_id,
                        } => (target_note_id.as_str(), parent_id.as_deref()),
                        ReviewNoteComposerKind::Reply { parent_id } => {
                            (composer.id.as_str(), Some(parent_id.as_str()))
                        }
                    };
                    project_extension_review_note(
                        ProjectableReviewNote {
                            id,
                            parent_id,
                            file_id: &file.runtime_id,
                            file_path: &file.path,
                            hunk_index: composer.target.hunk_index,
                            side: composer.target.side,
                            line: composer.target.line,
                            body: Some(&body),
                            summary: None,
                        },
                        draft,
                    )
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
            .filter(|_| self.options.cursor_line != CursorLineMode::Off)
            .copied()
            .or_else(|| {
                self.with_state(|state| {
                    let selection = state.selection();
                    let file = state.changeset().files.get(selection.file_index)?;
                    let hunk_index = selection.hunk_index?;
                    let hunk = file.hunks.get(hunk_index)?;
                    let (side, line) = selection
                        .side
                        .zip(selection.line)
                        .filter(|_| self.options.cursor_line != CursorLineMode::Off)
                        .unwrap_or_else(|| {
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
        let extension_note = self.extension_note_from_composer(&composer, body.to_owned(), false);
        let editing = matches!(composer.kind, ReviewNoteComposerKind::Edit { .. });
        let result = self.with_state(|state| match &composer.kind {
            ReviewNoteComposerKind::Edit { target_note_id, .. } => state
                .edit_comment_summary(target_note_id, body.to_owned())
                .map(|_| ()),
            ReviewNoteComposerKind::Create | ReviewNoteComposerKind::Reply { .. } => {
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
                    id: composer.id.clone(),
                    parent_id: match &composer.kind {
                        ReviewNoteComposerKind::Reply { parent_id } => Some(parent_id.clone()),
                        ReviewNoteComposerKind::Create | ReviewNoteComposerKind::Edit { .. } => {
                            None
                        }
                    },
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
            }
        });
        self.note_composer_bounds.set(None);
        self.focus = Focus::Review;
        match result {
            Ok(()) => {
                self.status = Some(if editing {
                    "review note updated".into()
                } else {
                    "review note saved".into()
                });
                if let Some(note) = extension_note {
                    self.publish_extension_lifecycle_event(if editing {
                        ExtensionLifecycleEvent::NoteEdited { note }
                    } else {
                        ExtensionLifecycleEvent::NoteCreated { note }
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
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::Review;
                self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
                    command_id: "workdeck.app.toggleFocusArea".into(),
                });
            }
            KeyCode::Enter => self.focus = Focus::Review,
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
        let can_apply_file_presentation_to_all_matching = self
            .file_presentation_menu_projection()
            .bulk_target
            .is_some();
        BuiltinCommandAvailability {
            can_align_current_line: self.options.cursor_line != CursorLineMode::Off,
            can_apply_file_presentation_to_all_matching,
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

    /// Publish one internally consistent review/selection/command projection before native code
    /// can observe it. This is Ratatui's synchronous counterpart to Hunk's layout-effect commit.
    fn commit_extension_runtime_bridge(&self) {
        let (snapshot, review, files, selected_file_id) = self.with_state(|state| {
            let snapshot = extension_runtime_bridge::SharedRuntimeSnapshot {
                generation: state.generation(),
                changeset: state.changeset_snapshot(),
                selection: state.selection(),
            };
            let files = self
                .extension_file_projection_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(state.changeset_snapshot());
            let selected_file_id = state
                .changeset()
                .files
                .get(state.selection().file_index)
                .map(|file| file.runtime_id.clone());
            (
                snapshot,
                build_extension_review_snapshot(state),
                files,
                selected_file_id,
            )
        });
        let mut selection = workdeck_extension_host::build_extension_review_selection_from_document(
            &snapshot.changeset,
            snapshot.selection,
        );
        if self.options.cursor_line == CursorLineMode::Off {
            selection.current_line = None;
        }
        self.extension_runtime_bridge
            .commit(ExtensionRuntimeCommit {
                registry_generation: self.extension_registry_generation,
                review_generation: self.extension_command_epoch,
                snapshot,
                review,
                files,
                selection,
                selected_file_id,
                commands: self.extension_command_availability(),
            });
    }

    fn app_menus(&self) -> AppMenus {
        let file_presentations = self.file_presentation_menu_projection();
        let builtins = self.builtin_commands();
        let mut commands = builtins
            .iter()
            .map(AppMenuCommand::from)
            .collect::<Vec<_>>();
        let (extension_commands, keyboard_mode_exit_entry, files_pane_visible) = {
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
            let visible_keys = runtime
                .layout
                .panes
                .iter()
                .map(|pane| pane.key.clone())
                .collect::<BTreeSet<_>>();
            let files_key = resolve_pane_slot_key(
                &runtime.session_panes,
                WORKDECK_FILES_PANE_KEY,
                &visible_keys,
                &runtime.failed_pane_registration_ids,
            );
            (
                extension_commands,
                keyboard_mode_exit_entry,
                visible_keys.contains(&files_key),
            )
        };
        commands.extend(extension_commands.iter().cloned());
        let file_view_apply_all_label = file_presentations
            .bulk_target
            .as_ref()
            .map(|target| format!("Apply \"{}\" to all matching files", target.title));
        build_app_menus(BuildAppMenusOptions {
            commands,
            extension_commands,
            file_view_entries: file_presentations.entries,
            keyboard_mode_exit_entry,
            file_view_apply_all_label,
            copy_decorations: self.copy_decorations,
            cursor_line: match self.options.cursor_line {
                CursorLineMode::Row => CommandCursorLine::Row,
                CursorLineMode::Number => CommandCursorLine::Number,
                CursorLineMode::Off => CommandCursorLine::Off,
            },
            layout_mode: self.layout(),
            files_pane_visible,
            show_agent_notes: self.options.agent_notes,
            show_help: self.show_help,
            show_hunk_headers: self.options.hunk_headers,
            show_line_numbers: self.options.line_numbers,
            show_menu_bar: self.show_menu_bar,
            wrap_lines: self.options.wrap_lines,
        })
    }

    fn review_file_is_visible(&self, files: &[DiffFile], file: &DiffFile) -> bool {
        files
            .iter()
            .position(|candidate| candidate.runtime_id == file.runtime_id)
            .is_some_and(|_| diff_file_matches_filter(file, &self.filter))
    }

    fn file_presentation_menu_projection(&self) -> FilePresentationMenuProjection {
        let (document, selected_index, draft_file_id) = self.with_state(|state| {
            let selection = state.selection();
            let draft_file_id = self.note_composer.as_ref().and_then(|composer| {
                state
                    .changeset()
                    .files
                    .get(composer.target.file_index)
                    .map(|file| file.runtime_id.clone())
            });
            (
                state.changeset_snapshot(),
                selection.file_index,
                draft_file_id,
            )
        });
        let files = &document.files;
        let Some(selected_file) = files.get(selected_index) else {
            return FilePresentationMenuProjection::default();
        };
        if !self.review_file_is_visible(files, selected_file) {
            return FilePresentationMenuProjection::default();
        }
        let unavailable_reason = file_view_unavailable_reason(
            draft_file_id.as_deref() == Some(selected_file.runtime_id.as_str()),
        );
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let registrations = runtime.file_views.clone();
        let candidates = registrations
            .iter()
            .map(|registration| FilePresentationMenuCandidate {
                key: registered_file_view_key(&registration.view),
                title: registration.title.clone(),
                matches: runtime.cached_file_view_matches(registration, selected_file),
            })
            .collect::<Vec<_>>();
        let stored_key = runtime
            .file_view_selections
            .get(&selected_file.runtime_id)
            .map(str::to_owned);
        let presented_key = if unavailable_reason.is_none() {
            stored_key
        } else {
            None
        };
        let bulk_target = presented_key.as_deref().and_then(|selected_key| {
            let registration = registrations.iter().find(|registration| {
                registered_file_view_key(&registration.view) == selected_key
            })?;
            let matching_file_ids = files
                .iter()
                .filter(|file| runtime.cached_file_view_matches(registration, file))
                .map(|file| file.runtime_id.clone())
                .collect::<Vec<_>>();
            (matching_file_ids
                .iter()
                .any(|file_id| file_id == &selected_file.runtime_id)
                && matching_file_ids
                    .iter()
                    .any(|file_id| runtime.file_view_selections.get(file_id) != Some(selected_key)))
            .then(|| FilePresentationBulkTarget {
                key: selected_key.to_owned(),
                title: registration.title.clone(),
                file_ids: matching_file_ids,
            })
        });
        plan_file_presentation_menu(
            Some(&selected_file.runtime_id),
            presented_key.as_deref(),
            unavailable_reason,
            &candidates,
            bulk_target,
        )
    }

    fn apply_file_presentation_bulk_target(&mut self) {
        let Some(target) = self.file_presentation_menu_projection().bulk_target else {
            return;
        };
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.file_view_selections = select_file_view_for_files(
            &runtime.file_view_selections,
            &target.file_ids,
            &target.key,
        );
        for file_id in &target.file_ids {
            clear_file_view_component_state(&mut runtime, file_id);
        }
        self.status = Some(format!(
            "file presentation: {} applied to matching files",
            target.title
        ));
    }

    fn set_file_presentation_for_file(&mut self, file_id: &str, view_key: Option<&str>) -> bool {
        let unchanged = self.selected_extension_file_view(file_id).as_deref() == view_key;
        if unchanged {
            return false;
        }
        self.exit_file_view_mode_for_file(file_id);
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.file_view_selections =
            select_file_view(&runtime.file_view_selections, file_id, view_key);
        clear_file_view_component_state(&mut runtime, file_id);
        true
    }

    fn select_current_file_presentation_from_menu(&mut self, view_key: Option<&str>) {
        let file_id = self.with_state(|state| {
            state
                .changeset()
                .files
                .get(state.selection().file_index)
                .map(|file| file.runtime_id.clone())
        });
        let Some(file_id) = file_id else {
            return;
        };
        self.set_file_presentation_for_file(&file_id, view_key);
        self.status = Some(match view_key {
            Some(view_key) => format!("file presentation: {view_key}"),
            None => "file presentation: raw diff".into(),
        });
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
            AppCommandAction::RequestQuit => self.request_quit(),
            AppCommandAction::ToggleHelp => {
                self.show_help = !self.show_help;
                if self.show_help {
                    self.show_agent_skill = false;
                    self.agent_skill_dialog_hits.set(None);
                    self.help_scroll = 0;
                } else {
                    self.help_dialog_hits.set(None);
                }
            }
            AppCommandAction::OpenAgentSkill => {
                self.show_help = false;
                self.help_dialog_hits.set(None);
                self.show_agent_skill = true;
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
                self.open_active_note_edit();
            }
            AppCommandAction::ReplyToActiveNote => {
                self.open_active_note_reply();
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
            AppCommandAction::ApplyFilePresentationToAllMatching => {
                self.apply_file_presentation_bulk_target();
            }
            AppCommandAction::ToggleFilesPane => self.toggle_files_pane_role(),
            AppCommandAction::RefreshCurrentInput => self.reload_requested = true,
            AppCommandAction::OpenThemeSelector => self.open_theme_selector(),
            AppCommandAction::ToggleAgentNotes => {
                self.options.agent_notes = !self.options.agent_notes;
            }
            AppCommandAction::ToggleLineNumbers => {
                self.options.line_numbers = !self.options.line_numbers;
            }
            AppCommandAction::ToggleLineWrap => {
                self.options.wrap_lines = !self.options.wrap_lines;
                self.options.horizontal_offset = 0;
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
        let rows = self.current_review_geometry_rows();
        let last = rows.lines.len().saturating_sub(1);
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(2 + u16::from(!self.options.pager))
                .max(1),
        );
        if unit == ScrollUnit::Content {
            if delta < 0 {
                self.scroll = 0;
                self.current_line_row = 0;
            } else {
                self.current_line_row = last;
                self.scroll = last.saturating_sub(viewport.saturating_sub(1));
            }
            self.synchronize_cursor_to_scrolled_viewport(&rows);
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
        let max_scroll = if self.review_geometry_published.get() {
            rows.lines.len().saturating_sub(viewport)
        } else {
            last
        };
        self.scroll = self
            .scroll
            .min(max_scroll)
            .saturating_add_signed(movement)
            .min(max_scroll);
        self.current_line_row = self.scroll.min(last);
        self.synchronize_cursor_to_scrolled_viewport(&rows);
    }

    fn synchronize_cursor_to_scrolled_viewport(&mut self, rows: &ReviewRows) {
        if self.options.cursor_line == CursorLineMode::Off {
            return;
        }
        let cursors = review_line_cursors(rows);
        if let Some(cursor) = cursors
            .iter()
            .copied()
            .find(|cursor| cursor.row >= self.current_line_row)
            .or_else(|| cursors.last().copied())
        {
            self.apply_review_line_cursor(cursor);
        }
    }

    fn toggle_files_pane_role(&mut self) {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut open = runtime.open.clone();
        let has_live_files_replacement = runtime.session_panes.iter().any(|pane| {
            pane.registered.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY)
                && !runtime
                    .failed_pane_registration_ids
                    .contains(&pane.registered.identity)
        });
        if (!has_live_files_replacement && self.options.sidebar)
            || runtime.force_builtin_files_sidebar
        {
            open.insert(WORKDECK_FILES_PANE_KEY.into());
        }
        let key = resolve_pane_slot_key(
            &runtime.session_panes,
            WORKDECK_FILES_PANE_KEY,
            &open,
            &runtime.failed_pane_registration_ids,
        );
        if key == WORKDECK_FILES_PANE_KEY {
            if runtime.force_builtin_files_sidebar {
                runtime.force_builtin_files_sidebar = false;
                self.options.sidebar = false;
                self.options.sidebar_visibility = SidebarVisibility::Hidden;
            } else {
                let responsive_shows_sidebar = !self.review_geometry_published.get()
                    || self.with_state(|state| {
                        state
                            .responsive_layout(self.review_width.get())
                            .show_sidebar
                    });
                let currently_visible = self.options.sidebar
                    && (responsive_shows_sidebar
                        || self.options.sidebar_visibility == SidebarVisibility::Visible);
                self.options.sidebar = !currently_visible;
                self.options.sidebar_visibility = if self.options.sidebar {
                    SidebarVisibility::Visible
                } else {
                    SidebarVisibility::Hidden
                };
            }
        } else if !runtime.open.remove(&key) {
            runtime.open.insert(key.clone());
            self.options.sidebar_visibility = SidebarVisibility::Visible;
        } else {
            self.options.sidebar = false;
            self.options.sidebar_visibility = SidebarVisibility::Hidden;
        }
        runtime.cached_renders.remove(&key);
    }

    fn step_diff_line(&mut self, delta: isize) {
        if self.options.cursor_line == CursorLineMode::Off {
            self.scroll_diff(delta, ScrollUnit::Step);
            return;
        }
        let rows = self.current_review_geometry_rows();
        let last = rows.lines.len().saturating_sub(1);
        let cursors = review_line_cursors(&rows);
        if cursors.is_empty() {
            self.current_line_row = self.current_line_row.saturating_add_signed(delta).min(last);
            let viewport = usize::from(
                self.review_height
                    .get()
                    .saturating_sub(2 + u16::from(!self.options.pager))
                    .max(1),
            );
            self.keep_current_line_visible(viewport, last);
            return;
        }
        let current = self.current_review_line_cursor_in(&cursors);
        let next = current.map_or_else(
            || {
                let next_index = 0usize
                    .saturating_add_signed(delta)
                    .min(cursors.len().saturating_sub(1));
                cursors[next_index]
            },
            |current| {
                let current_index = cursors
                    .iter()
                    .position(|candidate| *candidate == current)
                    .expect("current review cursor comes from the measured cursor list");
                let next_index = current_index
                    .saturating_add_signed(delta)
                    .min(cursors.len().saturating_sub(1));
                cursors[next_index]
            },
        );
        let changed = current != Some(next);
        self.apply_review_line_cursor(next);
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(2 + u16::from(!self.options.pager))
                .max(1),
        );
        self.keep_current_line_visible(viewport, last);
        if changed {
            self.publish_extension_selection_events();
        }
    }

    fn current_review_line_cursor(&self) -> Option<ReviewLineCursor> {
        let rows = self.current_review_geometry_rows();
        let cursors = review_line_cursors(&rows);
        self.current_review_line_cursor_in(&cursors)
    }

    /// Rebuild Hunk's selected split-row painter from the canonical Rust row plan.
    fn current_extension_line_paint(&self) -> Option<ExtensionCurrentLinePaint> {
        if self.options.cursor_line == CursorLineMode::Off
            || self.with_state(|state| state.resolved_layout(self.review_width.get()))
                != LayoutMode::Split
        {
            return None;
        }
        let cursor = self.current_review_line_cursor()?;
        let file = self.with_state(|state| {
            state
                .changeset()
                .files
                .get(cursor.target.file_index)
                .cloned()
        })?;
        let highlighted = if self.options.highlight {
            self.highlights
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .prefetch_highlighted_diff(&file, &self.options.theme, false)
        } else {
            None
        };
        let rows = build_split_rows(
            &file,
            highlighted.as_ref(),
            &self.options.theme,
            self.options.tab_width,
        );
        let planned_rows = build_review_render_plan(ReviewRenderPlanOptions {
            file_id: review_file_id(&file),
            rows: &rows,
            show_hunk_headers: self.options.hunk_headers,
            visible_agent_notes: &[],
            selected_hunk_index: Some(cursor.target.hunk_index),
            hunk_gap: usize::from(self.options.hunk_gap),
        })
        .into_iter()
        .filter_map(|planned| match planned {
            PlannedReviewRow::DiffRow {
                stable_key,
                stable_alias_keys,
                row,
                ..
            } => Some(CurrentLinePlannedRow {
                stable_key,
                stable_alias_keys,
                row,
            }),
            PlannedReviewRow::InlineNote { .. } | PlannedReviewRow::HunkGap { .. } => None,
        })
        .collect();
        let cursor = LineCursor {
            file_id: review_file_id(&file).to_owned(),
            hunk_index: cursor.target.hunk_index,
            stable_key: line_stable_key(
                cursor.target.hunk_index,
                cursor.target.side,
                usize::try_from(cursor.target.line).unwrap_or(usize::MAX),
            ),
            target: LineCursorTarget {
                side: cursor.target.side,
                line: cursor.target.line,
            },
            expanded_gap_key: None,
        };
        create_extension_current_line_paint(
            &cursor,
            &CurrentLineRowPlan {
                planned_rows,
                line_number_digits: self
                    .options
                    .line_number_digits
                    .unwrap_or_else(|| find_max_line_number(&file).to_string().len()),
            },
            self.options.line_numbers,
            self.options.horizontal_offset,
            &self.options.theme,
        )
    }

    fn current_review_line_cursor_in(
        &self,
        cursors: &[ReviewLineCursor],
    ) -> Option<ReviewLineCursor> {
        let selection = self.with_state(|state| state.selection());
        cursors
            .iter()
            .copied()
            .find(|cursor| {
                cursor.row == self.current_line_row
                    && cursor.target.file_index == selection.file_index
                    && cursor.target.hunk_index == selection.hunk_index.unwrap_or(0)
                    && selection
                        .side
                        .zip(selection.line)
                        .is_none_or(|(side, line)| {
                            cursor.target.side == side && cursor.target.line == line
                        })
            })
            .or_else(|| {
                selection.side.zip(selection.line).and_then(|(side, line)| {
                    cursors.iter().copied().find(|cursor| {
                        cursor.target.file_index == selection.file_index
                            && cursor.target.hunk_index == selection.hunk_index.unwrap_or(0)
                            && cursor.target.side == side
                            && cursor.target.line == line
                    })
                })
            })
            .or_else(|| {
                cursors.iter().copied().find(|cursor| {
                    cursor.target.file_index == selection.file_index
                        && cursor.target.hunk_index == selection.hunk_index.unwrap_or(0)
                })
            })
    }

    fn apply_review_line_cursor(&mut self, cursor: ReviewLineCursor) {
        self.current_line_row = cursor.row;
        let target = cursor.target;
        self.with_state(|state| {
            state
                .reveal_line(target.file_index, target.side, target.line)
                .or_else(|_| {
                    state.reveal_source_line(
                        target.file_index,
                        target.hunk_index,
                        target.side,
                        target.line,
                    )
                })
                .or_else(|error| {
                    let Some(file) = state.changeset().files.get(target.file_index) else {
                        return Err(error);
                    };
                    let Some(workdeck_review::ReviewSourceStatus::Loaded { text }) =
                        self.options.source_presentation.status(file)
                    else {
                        return Err(error);
                    };
                    let identity = file.source_identity.clone();
                    state.reveal_loaded_source_line(
                        target.file_index,
                        target.hunk_index,
                        target.side,
                        target.line,
                        identity.as_deref(),
                        text,
                    )
                })
                .expect("measured review cursor names a rendered diff line");
        });
    }

    fn seed_current_line_cursor(&mut self) {
        let rows = self.current_review_geometry_rows();
        let cursors = review_line_cursors(&rows);
        let selection = self.with_state(|state| state.selection());
        let cursor = cursors
            .iter()
            .copied()
            .find(|cursor| {
                cursor.target.file_index == selection.file_index
                    && cursor.target.hunk_index == selection.hunk_index.unwrap_or(0)
            })
            .or_else(|| {
                cursors
                    .iter()
                    .copied()
                    .find(|cursor| cursor.target.file_index == selection.file_index)
            })
            .or_else(|| cursors.first().copied());
        if let Some(cursor) = cursor {
            self.apply_review_line_cursor(cursor);
        }
    }

    fn align_current_line(&mut self, alignment: AppCommandLineAlignment) {
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(2 + u16::from(!self.options.pager))
                .max(1),
        );
        self.scroll = match alignment {
            AppCommandLineAlignment::Top => self.current_line_row,
            AppCommandLineAlignment::Center => self.current_line_row.saturating_sub(viewport / 2),
            AppCommandLineAlignment::Bottom => self
                .current_line_row
                .saturating_sub(viewport.saturating_sub(1)),
        };
    }

    fn theme_catalog(&self) -> Vec<AppTheme> {
        available_themes(&self.options.custom_themes)
    }

    fn apply_theme_id(&mut self, theme_id: &str) {
        let resolved = resolve_theme(Some(theme_id), None, &self.options.custom_themes);
        self.options.theme = if self.options.transparent_background {
            with_transparent_surfaces(&resolved)
        } else {
            resolved
        };
        self.themes
            .set_cursor_palette(Some(self.options.theme.id.clone()));
        self.highlights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn clear_theme_hover_preview(&mut self) {
        self.pending_theme_hover_preview = None;
        self.theme_selector_hovered_item_id = None;
    }

    fn open_theme_selector(&mut self) {
        let catalog = self.theme_catalog();
        self.themes.open_selector(&catalog);
        self.clear_theme_hover_preview();
        *self
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        self.show_help = false;
        self.help_dialog_hits.set(None);
        self.show_agent_skill = false;
        self.agent_skill_dialog_hits.set(None);
    }

    fn close_theme_selector(&mut self) {
        let committed = self.themes.close_selector();
        self.clear_theme_hover_preview();
        *self
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        self.apply_theme_id(&committed);
    }

    fn move_theme_selector(&mut self, delta: isize) {
        let catalog = self.theme_catalog();
        self.retain_rendered_theme_window(&catalog);
        self.clear_theme_hover_preview();
        if let Some(theme_id) = self.themes.move_selector(&catalog, delta) {
            self.apply_theme_id(&theme_id);
        }
    }

    fn preview_theme_selector_item(&mut self, index: usize) {
        let catalog = self.theme_catalog();
        self.retain_rendered_theme_window(&catalog);
        if let Some(theme_id) = self.themes.preview_index(&catalog, index) {
            self.apply_theme_id(&theme_id);
        }
    }

    fn retain_rendered_theme_window(&mut self, catalog: &[AppTheme]) {
        let plan = self
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(plan) = plan.as_ref()
            && plan.window.selected_index == self.themes.selected_index(catalog)
            && plan.window.item_count == catalog.len()
        {
            // Render takes an immutable app reference. Carry its actual window
            // into the next transition instead of recentering on every key.
            self.themes.window = Some(plan.window);
        }
    }

    fn accept_theme_selector(&mut self) {
        let catalog = self.theme_catalog();
        let accepted = self.themes.accept_selector(&catalog);
        self.finish_theme_selector_acceptance(accepted);
    }

    fn accept_theme_selector_item(&mut self, index: usize) {
        let catalog = self.theme_catalog();
        let accepted = self.themes.accept_selector_index(&catalog, index);
        self.finish_theme_selector_acceptance(accepted);
    }

    fn finish_theme_selector_acceptance(&mut self, accepted: Option<(String, String)>) {
        let Some((theme_id, label)) = accepted else {
            return;
        };
        self.clear_theme_hover_preview();
        *self
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        self.apply_theme_id(&theme_id);
        self.status = Some(format!("Theme: {label}"));
    }

    fn scroll_theme_selector_window(&mut self, delta: isize) {
        let plan = self
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(plan) = plan else {
            return;
        };
        self.clear_theme_hover_preview();
        let max_start = plan
            .window
            .item_count
            .saturating_sub(plan.window.visible_rows);
        let window_start = plan
            .window
            .window_start
            .saturating_add_signed(delta)
            .min(max_start);
        self.themes.window = Some(ThemeSelectorWindowState {
            window_start,
            ..plan.window
        });
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
                // Keep the document selection even when hidden, but walk only
                // the visible stream, matching core selectReviewNavigationFiles.
                .filter(|file| diff_file_matches_filter(file, &self.filter))
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
        let changed = self.with_state(|state| {
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
        if changed {
            self.reconcile_active_file_view_mode();
            self.scroll_to_reveal(target.reveal);
            self.publish_extension_selection_events();
        }
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
        self.commit_extension_runtime_bridge();
        let command_epoch = self.extension_command_epoch;
        let committed = self.extension_runtime_bridge.committed_review();
        let selection = self.extension_runtime_bridge.get_selection();
        let review_controls = self.extension_runtime_bridge.create_review_controls();
        let files = self.with_state(|state| state.changeset_snapshot());
        let workspace = self
            .options
            .review_input
            .as_ref()
            .zip(self.options.repo.as_deref())
            .map(|(input, root)| {
                let files = files
                    .files
                    .iter()
                    .map(|file| {
                        self.options.source_capabilities.as_ref().map_or_else(
                            || Ok(file.clone()),
                            |capabilities| capabilities.with_source_snapshots(file),
                        )
                    })
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok::<_, workdeck_vcs::VcsCatalogError>(build_extension_workspace_snapshot(
                    &files,
                    input,
                    root,
                    command_epoch,
                ))
            })
            .transpose();
        let workspace = match workspace {
            Ok(workspace) => workspace,
            Err(error) => {
                self.status = Some(format!("Extension workspace source unavailable: {error}"));
                return;
            }
        };
        let snapshot = committed.snapshot;
        let review = review_controls.snapshot().unwrap_or(committed.review);
        let cwd = self.extension_command_cwd();
        let commands = self
            .extension_runtime_bridge
            .command_controls()
            .availability();
        let navigation = self
            .extension_runtime_bridge
            .create_navigation(command.extension_id.clone());
        let selected_file_id = selection.file.as_ref().map(|file| file.id.as_str());
        let presented_file_view =
            selected_file_id.and_then(|file_id| self.presented_extension_file_view(file_id));
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
            let own_view_prefix = format!("{}:", command.extension_id);
            let file_views = ExtensionFileViewContext {
                active_view_id: presented_file_view
                    .as_deref()
                    .and_then(|key| key.strip_prefix(&own_view_prefix))
                    .map(str::to_owned),
                active_mode_id: runtime
                    .active_file_view_mode
                    .as_ref()
                    .filter(|active| active.extension_id == command.extension_id)
                    .map(|active| active.view_id.clone()),
            };
            runtime
                .request_queues
                .entry(command.extension_index)
                .or_default()
                .push_back(QueuedExtensionRequest::Command(Box::new(
                    QueuedExtensionCommand {
                        pending: PendingExtensionCommand {
                            extension_index: command.extension_index,
                            extension_id: command.extension_id.clone(),
                            command_id: command.command.id.clone(),
                            title: command.command.title.clone(),
                            review_generation: command_epoch,
                            navigation,
                        },
                        snapshot,
                        selection,
                        open_panes,
                        active_keyboard_mode,
                        cwd,
                        review,
                        commands,
                        workspace,
                        file_views,
                    },
                )));
        }
        self.status = Some(command.command.title);
        self.start_queued_extension_requests(Some(command.extension_index));
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
                    self.apply_extension_command_actions(&pending, execution.actions);
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
            // Hunk observes the extension command after its synchronous handler returns. Native
            // commands cross a subprocess boundary, so publish after applying the returned host
            // actions: programmatic built-ins are observed first and the extension command once.
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
                command_id: format!("{}.{}", pending.extension_id, pending.command_id),
            });
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
        // Background file-view/highlight work can temporarily own a connection
        // without creating a pending command/event here. Retry its deferred queue
        // on every tick, not only when another command or event completes.
        self.start_queued_extension_requests(None);
        if let Some((activation, key)) = self.deferred_file_view_keys.pop_front() {
            let current = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .active_file_view_mode
                .as_ref()
                .map(|mode| mode.activation_id);
            if current.is_none() || current == Some(activation) {
                self.replaying_file_view_key = true;
                self.handle_key(key);
                self.replaying_file_view_key = false;
            }
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
                            .begin_command_with_complete_context(
                                &queued.pending.command_id,
                                queued.snapshot,
                                queued.selection,
                                queued.open_panes,
                                queued.active_keyboard_mode,
                                queued.cwd,
                                Some(queued.review),
                                queued.commands,
                                queued.workspace,
                                queued.file_views,
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
        pending: &PendingExtensionCommand,
        actions: Vec<ExtensionHostAction>,
    ) {
        let current_generation = self.extension_command_epoch;
        let review_authority_live =
            pending.navigation.is_live() && current_generation == pending.review_generation;
        let mut live_actions = Vec::with_capacity(actions.len());
        for action in actions {
            if !review_authority_live {
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
            if !review_authority_live
                && let ExtensionHostAction::RequestWorkspaceRead { request_id, .. } = action
            {
                self.complete_extension_workspace_read(
                    pending.extension_index,
                    &pending.extension_id,
                    request_id,
                    None,
                );
                continue;
            }
            if !review_authority_live
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

    fn handle_focused_extension_pane_input(&mut self, key: &KeyEvent) -> bool {
        let active = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let valid = runtime.focused_pane_input.as_ref().is_some_and(|active| {
                runtime.open.contains(&active.pane_key)
                    && runtime.panes.iter().any(|pane| {
                        pane.key == active.pane_key
                            && pane.registered.identity == active.registration_identity
                    })
            });
            if !valid {
                runtime.focused_pane_input = None;
                return false;
            }
            runtime.focused_pane_input.clone().expect("validated above")
        };

        let mut value = active.value.clone();
        let mut cursor = active.cursor.min(value.chars().count());
        let changed = match key.code {
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                insert_filter_character(&mut value, &mut cursor, character);
                true
            }
            KeyCode::Backspace => {
                let before = value.clone();
                remove_filter_character_before(&mut value, &mut cursor);
                value != before
            }
            KeyCode::Delete => {
                let before = value.clone();
                remove_filter_character_at(&mut value, &mut cursor);
                value != before
            }
            KeyCode::Left => {
                cursor = cursor.saturating_sub(1);
                false
            }
            KeyCode::Right => {
                cursor = cursor.saturating_add(1).min(value.chars().count());
                false
            }
            KeyCode::Home => {
                cursor = 0;
                false
            }
            KeyCode::End => {
                cursor = value.chars().count();
                false
            }
            // The live pane input remains the focus authority for modified
            // shortcuts too. It may not edit its one-line value for this key,
            // but no review command, extension mode, or job-control handler may
            // act behind it.
            _ => return true,
        };

        if !changed {
            if let Some(current) = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .focused_pane_input
                .as_mut()
                .filter(|current| {
                    current.pane_key == active.pane_key && current.input_id == active.input_id
                })
            {
                current.cursor = cursor;
            }
            return true;
        }

        let (snapshot, review) =
            self.with_state(|state| (state.snapshot(), build_extension_review_snapshot(state)));
        let cwd = self.extension_command_cwd();
        let execution = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let open_panes = runtime.open.iter().cloned().collect();
            runtime.extensions[active.extension_index].invoke_pane_input(PaneInputInvocation {
                pane_id: active.pane_id.clone(),
                input_id: active.input_id.clone(),
                value: value.clone(),
                snapshot,
                cwd,
                review: Some(review),
                open_panes,
            })
        };
        match execution {
            Ok(execution) => {
                {
                    let mut runtime = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(current) = runtime.focused_pane_input.as_mut().filter(|current| {
                        current.pane_key == active.pane_key
                            && current.registration_identity == active.registration_identity
                            && current.input_id == active.input_id
                    }) {
                        current.value = value;
                        current.cursor = cursor;
                    }
                    runtime.cached_renders.remove(&active.pane_key);
                }
                self.apply_extension_actions(
                    active.extension_index,
                    &active.extension_id,
                    execution.actions,
                );
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {} pane input failed: {error}",
                    active.extension_id
                ));
            }
        }
        true
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
                ExtensionHostAction::TogglePane { id } => {
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
                    if !runtime.open.remove(&pane_key) {
                        runtime.open.insert(pane_key.clone());
                    }
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
                ExtensionHostAction::SelectFileView { id } => {
                    self.select_extension_file_view(extension_index, extension_id, id.as_deref());
                }
                ExtensionHostAction::EnterFileViewMode { id } => {
                    self.enter_file_view_mode(extension_index, extension_id, &id);
                }
                ExtensionHostAction::ExitFileViewMode => {
                    self.exit_active_file_view_mode();
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
                ExtensionHostAction::RequestWorkspaceRead {
                    request_id,
                    file_id,
                    side,
                } => {
                    let value = self.with_state(|state| {
                        resolve_extension_workspace_read(&file_id, &state.changeset().files, side)
                            .map(str::to_owned)
                    });
                    self.complete_extension_workspace_read(
                        extension_index,
                        extension_id,
                        request_id,
                        value,
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
                    let message = sanitize_terminal_line(&message);
                    if let Some(notifications) = self.options.extension_notifications.as_ref() {
                        notifications.notify(message, notification_type);
                    } else {
                        let severity = match notification_type {
                            ExtensionNotifyType::Info => "",
                            ExtensionNotifyType::Warning => "warning: ",
                            ExtensionNotifyType::Error => "error: ",
                        };
                        self.status = Some(format!("{extension_id}: {severity}{message}"));
                    }
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
        self.commit_extension_runtime_bridge();
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
                .push_back(QueuedExtensionRequest::Event(Box::new(
                    QueuedExtensionEvent {
                        event,
                        dispatch_depth: self.extension_event_dispatch_depth,
                    },
                )));
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
        // Committed extension projections advance even when no current extension subscribes to
        // the resulting lifecycle events; retained controls must never observe stale selection.
        self.commit_extension_runtime_bridge();
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
        self.extension_confirm_hovered_action_key = None;
        self.extension_dialog_hovered_action_key = None;
        *self
            .extension_confirm_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        *self
            .extension_input_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        *self
            .extension_select_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
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
        self.extension_confirm_hovered_action_key = None;
        self.extension_dialog_hovered_action_key = None;
        *self
            .extension_confirm_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        *self
            .extension_input_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        *self
            .extension_select_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
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
        match outcome {
            Ok(execution) => {
                self.apply_extension_actions(extension_index, &extension_id, execution.actions);
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {extension_id} dialog retirement failed: {error}"
                ));
            }
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

    fn complete_extension_workspace_read(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        request_id: String,
        value: Option<String>,
    ) {
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[extension_index]
            .complete_workspace_read(ExtensionWorkspaceReadCompletion { request_id, value });
        match execution {
            Ok(execution) => {
                self.apply_extension_actions(extension_index, extension_id, execution.actions)
            }
            Err(error) => {
                self.status = Some(format!(
                    "extension {extension_id} workspace read completion failed: {error}"
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
        // Match AppHost's atomic `runWorkspaceWrite` boundary: shutdown may
        // refuse work that has not started, while a write already executing
        // reports its real result and is synchronously drained by this owner
        // thread before teardown can continue.
        if self.shutdown_requested() {
            return Err(WorkspaceWriteFailure::Unavailable(
                "The review reloaded before this extension operation could finish.".into(),
            ));
        }
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
        let file = self
            .options
            .source_capabilities
            .as_ref()
            .map_or_else(
                || Ok(file.clone()),
                |capabilities| capabilities.with_source_snapshots(&file),
            )
            .map_err(|error| WorkspaceWriteFailure::Unavailable(error.to_string()))?;
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
            self.settle_current_extension_confirm(confirmed);
        }
        true
    }

    fn settle_current_extension_confirm(&mut self, confirmed: bool) {
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

    fn settle_current_extension_select(&mut self, selected_index: Option<usize>, accept: bool) {
        let settlement = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(selected_index) = selected_index {
                runtime.dialogs.pick_option(selected_index);
            }
            let selected = runtime.dialogs.current().and_then(|request| match request {
                ExtensionDialogRequest::Select(dialog) => Some((
                    dialog.request_id,
                    dialog.options.get(dialog.selected).cloned(),
                )),
                _ => None,
            });
            selected.and_then(|(request_id, value)| {
                if accept {
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

    fn settle_current_extension_input(&mut self, accept: bool) {
        let settlement = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let input = runtime.dialogs.current().and_then(|request| match request {
                ExtensionDialogRequest::Input(dialog) => {
                    Some((dialog.request_id, Some(dialog.value.clone())))
                }
                _ => None,
            });
            input.and_then(|(request_id, value)| {
                if accept {
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

    fn handle_extension_select_mouse(&mut self, event: &MouseEvent) -> bool {
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
        let plan = self
            .extension_select_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(plan) = plan else {
            return true;
        };
        match event.kind {
            MouseEventKind::Moved => {
                self.extension_dialog_hovered_action_key =
                    extension_dialog_action_at(&plan.action_hits, event.column, event.row)
                        .map(|hit| hit.key_label.clone());
            }
            MouseEventKind::Up(_) => {
                if let Some(hit) = select_item_at(&plan, event.column, event.row) {
                    self.settle_current_extension_select(Some(hit.index), true);
                } else if let Some(hit) =
                    extension_dialog_action_at(&plan.action_hits, event.column, event.row)
                {
                    self.settle_current_extension_select(None, hit.index == 0);
                } else if plan
                    .modal
                    .close
                    .is_some_and(|close| rect_contains(close, event.column, event.row))
                    || !rect_contains(plan.modal.frame, event.column, event.row)
                {
                    self.settle_current_extension_select(None, false);
                }
            }
            _ => {}
        }
        true
    }

    fn handle_extension_input_mouse(&mut self, event: &MouseEvent) -> bool {
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
        let plan = self
            .extension_input_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(plan) = plan else {
            return true;
        };
        match event.kind {
            MouseEventKind::Moved => {
                self.extension_dialog_hovered_action_key =
                    extension_dialog_action_at(&plan.action_hits, event.column, event.row)
                        .map(|hit| hit.key_label.clone());
            }
            MouseEventKind::Up(_) => {
                if let Some(hit) =
                    extension_dialog_action_at(&plan.action_hits, event.column, event.row)
                {
                    self.settle_current_extension_input(hit.index == 0);
                } else if plan
                    .modal
                    .close
                    .is_some_and(|close| rect_contains(close, event.column, event.row))
                    || !rect_contains(plan.modal.frame, event.column, event.row)
                {
                    self.settle_current_extension_input(false);
                }
            }
            _ => {}
        }
        true
    }

    fn handle_extension_confirm_mouse(&mut self, event: &MouseEvent) -> bool {
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
        let map = self
            .extension_confirm_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(map) = map else {
            return true;
        };
        match event.kind {
            MouseEventKind::Moved => {
                self.extension_confirm_hovered_action_key =
                    dialog_action_at(&map, event.column, event.row)
                        .map(|hit| hit.key_label.clone());
            }
            MouseEventKind::Up(_) => {
                if let Some(hit) = dialog_action_at(&map, event.column, event.row) {
                    self.settle_current_extension_confirm(hit.index == 0);
                } else if map
                    .modal
                    .close
                    .is_some_and(|close| rect_contains(close, event.column, event.row))
                    || !rect_contains(map.modal.frame, event.column, event.row)
                {
                    self.settle_current_extension_confirm(false);
                }
            }
            _ => {}
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
        if self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions
            .len()
            <= dialog.extension_index
        {
            self.status = Some(format!(
                "extension {} input retired before submission",
                dialog.extension_id
            ));
            return;
        }
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
        if self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions
            .len()
            <= dialog.extension_index
        {
            self.status = Some(format!(
                "extension {} selection retired before submission",
                dialog.extension_id
            ));
            return;
        }
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
        if self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions
            .len()
            <= dialog.extension_index
        {
            self.status = Some(format!(
                "extension {} confirmation retired before submission",
                dialog.extension_id
            ));
            return;
        }
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
        self.scroll_to_reveal(workdeck_review::REVIEW_FILE_JUMP_REVEAL);
        self.publish_extension_selection_events();
    }

    fn toggle_extension_file_view(
        &mut self,
        extension_index: usize,
        extension_id: &str,
        view_id: &str,
    ) {
        let file_id = self.with_state(|state| {
            state
                .changeset()
                .files
                .get(state.selection().file_index)
                .map(|file| file.runtime_id.clone())
        });
        let registration = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            resolve_live_file_view(&runtime.file_views, extension_id, view_id)
        };
        let active = file_id.as_deref().is_some_and(|file_id| {
            registration.as_ref().is_some_and(|registration| {
                let view_key = registered_file_view_key(&registration.view);
                self.presented_extension_file_view(file_id).as_deref() == Some(view_key.as_str())
            })
        });
        self.select_extension_file_view(
            extension_index,
            extension_id,
            (!active).then_some(view_id),
        );
    }

    fn select_extension_file_view(
        &mut self,
        _extension_index: usize,
        extension_id: &str,
        view_id: Option<&str>,
    ) {
        let (files, file, draft_file_id) = self.with_state(|state| {
            let file = state
                .changeset()
                .files
                .get(state.selection().file_index)
                .cloned();
            let draft_file_id = self.note_composer.as_ref().and_then(|composer| {
                state
                    .changeset()
                    .files
                    .get(composer.target.file_index)
                    .map(|file| file.runtime_id.clone())
            });
            (state.changeset().files.clone(), file, draft_file_id)
        });
        let Some(file) = file else {
            self.status = Some(format!(
                "Extension {extension_id} cannot select a file view without a selected file"
            ));
            return;
        };
        let Some(view_id) = view_id else {
            self.set_file_presentation_for_file(&file.runtime_id, None);
            self.status = Some("file presentation: raw diff".into());
            return;
        };
        if let Some(reason) =
            file_view_unavailable_reason(draft_file_id.as_deref() == Some(file.runtime_id.as_str()))
        {
            self.status = Some(reason.into());
            return;
        }
        let registration = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            resolve_live_file_view(&runtime.file_views, extension_id, view_id)
        };
        let Some(registration) = registration else {
            self.status = Some(format!(
                "Extension {extension_id} targeted unknown file view \"{view_id}\""
            ));
            return;
        };
        if !self.review_file_is_visible(&files, &file) {
            self.status = Some(format!(
                "File view \"{view_id}\" does not match the selected file • using raw diff"
            ));
            return;
        }
        let view_key = registered_file_view_key(&registration.view);
        let snapshot = create_file_view_input_snapshot(&file);
        let matches = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[registration.extension_index]
            .file_view_matches(&registration.view.view_id, snapshot.file.as_ref().clone());
        match matches {
            Ok(true) => {
                self.set_file_presentation_for_file(&file.runtime_id, Some(&view_key));
                self.status = Some(format!("file presentation: {view_key}"));
            }
            Ok(false) => {
                self.status = Some(format!(
                    "File view \"{view_id}\" does not match the selected file • using raw diff"
                ));
            }
            Err(_) => {
                self.status = Some(format!(
                    "Extension {} file view \"{}\" failed matching the selected file",
                    registration.view.extension_id, registration.view.view_id
                ));
            }
        }
    }

    fn enter_file_view_mode(&mut self, extension_index: usize, extension_id: &str, view_id: &str) {
        if self.file_view_mode_transition_depth >= MAX_FILE_VIEW_MODE_TRANSITION_DEPTH {
            self.status = Some(format!(
                "Extension {extension_id} exceeded the file-view mode transition limit"
            ));
            return;
        }
        self.file_view_mode_transition_depth += 1;
        self.enter_file_view_mode_inner(extension_index, extension_id, view_id);
        self.file_view_mode_transition_depth -= 1;
    }

    fn enter_file_view_mode_inner(
        &mut self,
        _extension_index: usize,
        extension_id: &str,
        view_id: &str,
    ) {
        let (files, file, review_generation) = self.with_state(|state| {
            (
                state.changeset().files.clone(),
                state
                    .changeset()
                    .files
                    .get(state.selection().file_index)
                    .cloned(),
                state.generation(),
            )
        });
        let Some(file) = file else {
            self.status = Some(format!(
                "Extension {extension_id} cannot enter a mode without a selected file"
            ));
            return;
        };
        if !self.review_file_is_visible(&files, &file) {
            self.status = Some(format!(
                "Extension {extension_id} cannot enter a mode without a selected file"
            ));
            return;
        }
        let draft_file_id = self.with_state(|state| {
            self.note_composer.as_ref().and_then(|composer| {
                state
                    .changeset()
                    .files
                    .get(composer.target.file_index)
                    .map(|file| file.runtime_id.clone())
            })
        });
        if let Some(reason) =
            file_view_unavailable_reason(draft_file_id.as_deref() == Some(file.runtime_id.as_str()))
        {
            self.status = Some(reason.into());
            return;
        }
        let registration = {
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            resolve_live_file_view(&runtime.file_views, extension_id, view_id)
        };
        let Some(registration) = registration else {
            self.status = Some(format!(
                "Extension {extension_id} targeted unknown file view \"{view_id}\""
            ));
            return;
        };
        if !registration.view.interactive_mode {
            self.status = Some(format!(
                "Extension {extension_id} file view \"{view_id}\" has no interactive mode"
            ));
            return;
        }
        let snapshot = create_file_view_input_snapshot(&file);
        let owner_index = registration.extension_index;
        let owner_id = registration.view.extension_id.clone();
        let owner_view_id = registration.view.view_id.clone();
        let matches = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[owner_index]
            .file_view_matches(&owner_view_id, snapshot.file.as_ref().clone());
        match matches {
            Ok(true) => {}
            Ok(false) => {
                self.status = Some(format!(
                    "File view \"{view_id}\" does not match the selected file • using raw diff"
                ));
                return;
            }
            Err(_) => {
                self.status = Some(format!(
                    "Extension {owner_id} file view \"{owner_view_id}\" failed matching the selected file"
                ));
                return;
            }
        }

        self.exit_active_file_view_mode();
        let current_activation_id = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .as_ref()
            .map(|active| active.activation_id);
        if !requested_mode_may_enter(current_activation_id) {
            return;
        }
        let view_key = registered_file_view_key(&registration.view);
        let activation_id = {
            let mut runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.next_file_view_mode_activation_id =
                runtime.next_file_view_mode_activation_id.saturating_add(1);
            runtime.next_file_view_mode_activation_id
        };
        let active = ActiveFileViewModeRuntime {
            activation_id,
            extension_index: owner_index,
            extension_id: owner_id.clone(),
            view_id: owner_view_id.clone(),
            view_key: view_key.clone(),
            registration_identity: registration.registration_identity,
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
            clear_file_view_component_state(&mut runtime, &file.runtime_id);
            runtime.active_file_view_mode = Some(active.clone());
        }
        let request = self.file_view_mode_lifecycle_request(&active);
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[owner_index]
            .file_view_mode_lifecycle("workdeck/file-view-mode/enter", request);
        match execution {
            Ok(execution) => {
                self.apply_extension_actions(owner_index, &owner_id, execution.actions);
                let current_activation_id = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .active_file_view_mode
                    .as_ref()
                    .map(|current| current.activation_id);
                if let Some(detail) = execution.failure {
                    if activation_still_owns_mode(current_activation_id, activation_id) {
                        self.exit_active_file_view_mode();
                    }
                    self.report_file_view_mode_failure(
                        &owner_id,
                        &owner_view_id,
                        "onEnter",
                        &detail,
                    );
                } else if activation_still_owns_mode(current_activation_id, activation_id) {
                    self.status = Some(format!("{owner_id}:{owner_view_id} mode — Esc exits"));
                }
            }
            Err(error) => {
                let current_activation_id = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .active_file_view_mode
                    .as_ref()
                    .map(|current| current.activation_id);
                if activation_still_owns_mode(current_activation_id, activation_id) {
                    self.exit_active_file_view_mode();
                }
                let detail = extension_command_error_detail(&error);
                self.report_file_view_mode_failure(&owner_id, &owner_view_id, "onEnter", &detail);
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

    fn report_file_view_mode_failure(
        &mut self,
        extension_id: &str,
        view_id: &str,
        action: &str,
        detail: &str,
    ) {
        let detail = sanitize_terminal_line(detail);
        let message =
            file_view_mode_failure_message(extension_id, view_id, action, detail.as_str());
        if let Some(notifications) = self.options.extension_notifications.as_ref() {
            notifications.notify(message.clone(), ExtensionNotifyType::Warning);
        }
        self.status = Some(message);
    }

    fn route_active_file_view_mode(&mut self, key: &KeyEvent) -> bool {
        self.reconcile_active_file_view_mode();
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
                    let current_activation_id = self
                        .extension_pane_runtime
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .active_file_view_mode
                        .as_ref()
                        .map(|current| current.activation_id);
                    if activation_still_owns_mode(current_activation_id, active.activation_id) {
                        self.exit_active_file_view_mode();
                    }
                }
                routing != KeyRoutingResult::Pass
            }
            Err(HostError::Busy(_)) => {
                // Background presentation work shares the ordered native connection.
                // Retry this key before later input rather than dropping it or retiring
                // a healthy mode; activation identity prevents delivery to a new mode.
                self.deferred_file_view_keys
                    .push_front((active.activation_id, *key));
                true
            }
            Err(error) => {
                let current_activation_id = self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .active_file_view_mode
                    .as_ref()
                    .map(|current| current.activation_id);
                if activation_still_owns_mode(current_activation_id, active.activation_id) {
                    self.exit_active_file_view_mode();
                }
                let detail = extension_command_error_detail(&error);
                self.report_file_view_mode_failure(
                    &active.extension_id,
                    &active.view_id,
                    "onKey",
                    &detail,
                );
                true
            }
        }
    }

    fn exit_active_file_view_mode(&mut self) {
        if self.file_view_mode_transition_depth >= MAX_FILE_VIEW_MODE_TRANSITION_DEPTH {
            self.status = Some("File-view mode transition limit exceeded".into());
            return;
        }
        self.file_view_mode_transition_depth += 1;
        self.exit_active_file_view_mode_inner();
        self.file_view_mode_transition_depth -= 1;
    }

    fn exit_active_file_view_mode_inner(&mut self) {
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
        let mode_status = format!(
            "{}:{} mode — Esc exits",
            active.extension_id, active.view_id
        );
        if self.status.as_deref() == Some(mode_status.as_str()) {
            self.status = None;
        }
        let request = self.file_view_mode_lifecycle_request(&active);
        let execution = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .extensions[active.extension_index]
            .file_view_mode_lifecycle("workdeck/file-view-mode/exit", request);
        match execution {
            Ok(execution) => {
                self.apply_extension_actions(
                    active.extension_index,
                    &active.extension_id,
                    execution.actions,
                );
                if let Some(detail) = execution.failure {
                    self.report_file_view_mode_failure(
                        &active.extension_id,
                        &active.view_id,
                        "onExit",
                        &detail,
                    );
                }
            }
            Err(error) => {
                let detail = extension_command_error_detail(&error);
                self.report_file_view_mode_failure(
                    &active.extension_id,
                    &active.view_id,
                    "onExit",
                    &detail,
                );
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

    fn exit_file_view_mode_for_file(&mut self, file_id: &str) {
        let owns = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .as_ref()
            .is_some_and(|active| active.file.id == file_id);
        if owns {
            self.exit_active_file_view_mode();
        }
    }

    fn refresh_extension_file_view(
        &mut self,
        _extension_index: usize,
        extension_id: &str,
        view_id: &str,
        file_id: Option<&str>,
    ) {
        if file_id.is_some_and(|file_id| {
            !self.with_state(|state| {
                state
                    .changeset()
                    .files
                    .iter()
                    .any(|file| file.runtime_id == file_id)
            })
        }) {
            // A scoped id may race a reload. Hunk deliberately treats it as a
            // quiet no-op: it cannot invalidate a layout the review owns.
            return;
        }
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(registration) = resolve_live_file_view(&runtime.file_views, extension_id, view_id)
        else {
            self.status = Some(format!(
                "extension {extension_id} targeted unknown file view {view_id:?}"
            ));
            return;
        };
        let view_key = registered_file_view_key(&registration.view);
        runtime.file_view_epochs = workdeck_extension_host::bump_file_view_epoch(
            &runtime.file_view_epochs,
            &view_key,
            file_id,
        );
        runtime.file_view_layouts.invalidate(&view_key, file_id);
        if let Some(file_id) = file_id {
            clear_file_view_component_state(&mut runtime, file_id);
        } else {
            let selected = runtime
                .file_view_selections
                .entries()
                .iter()
                .filter_map(|(file_id, selected)| {
                    (selected == &view_key).then_some(file_id.clone())
                })
                .collect::<Vec<_>>();
            for file_id in selected {
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

    #[must_use]
    pub fn presented_extension_file_view(&self, file_id: &str) -> Option<String> {
        let draft_file_id = self.with_state(|state| {
            self.note_composer.as_ref().and_then(|composer| {
                state
                    .changeset()
                    .files
                    .get(composer.target.file_index)
                    .map(|file| file.runtime_id.clone())
            })
        });
        if draft_file_id.as_deref() == Some(file_id) {
            return None;
        }
        self.selected_extension_file_view(file_id)
    }

    fn prepare_extension_file_view_layouts(
        &self,
        changeset: &Changeset,
        width: u16,
    ) -> BTreeMap<String, ResolvedFileViewLayout> {
        let width = usize::from(width.max(1));
        let draft_file_id = self.note_composer.as_ref().and_then(|composer| {
            changeset
                .files
                .get(composer.target.file_index)
                .map(|file| file.runtime_id.clone())
        });
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let selections = runtime.file_view_selections.entries().clone();
        let registrations = runtime.file_views.clone();
        let mut candidates = BTreeMap::new();
        for file in &changeset.files {
            if !diff_file_matches_filter(file, &self.filter) {
                continue;
            }
            if draft_file_id.as_deref() == Some(file.runtime_id.as_str()) {
                continue;
            }
            let Some(view_key) = selections.get(&file.runtime_id) else {
                continue;
            };
            let Some(registration) = registrations
                .iter()
                .find(|registration| registered_file_view_key(&registration.view) == *view_key)
            else {
                continue;
            };
            let identity = FileViewLayoutIdentity {
                file_id: file.runtime_id.clone(),
                file_path: file.path.clone(),
                content_identity: file.content_identity.clone(),
                view_key: view_key.clone(),
                extension_id: registration.view.extension_id.clone(),
                view_id: registration.view.view_id.clone(),
                registration_identity: registration.registration_identity,
                width,
                epoch: workdeck_extension_host::file_view_layout_epoch(
                    &runtime.file_view_epochs,
                    view_key,
                    &file.runtime_id,
                ),
            };
            candidates.insert(identity, (file.clone(), registration.clone()));
        }
        let warnings = runtime.file_view_layouts.poll_results();
        let identities = candidates.keys().cloned().collect::<Vec<_>>();
        let prepared = runtime
            .file_view_layouts
            .reconcile(Instant::now(), identities);
        let tasks = runtime.file_view_layouts.take_startable();
        let sender = runtime.file_view_layouts.result_sender();
        let mut dispatches = Vec::new();
        for task in tasks {
            let Some((file, registration)) = candidates.get(&task.identity) else {
                task.cancellation.cancel();
                continue;
            };
            let Some(extension) = runtime
                .extensions
                .get(registration.extension_index)
                .cloned()
            else {
                task.cancellation.cancel();
                continue;
            };
            clear_file_view_component_state(&mut runtime, &file.runtime_id);
            dispatches.push(FileViewLayoutDispatch {
                task,
                file: file.clone(),
                extension,
                view_id: registration.view.view_id.clone(),
                source_capabilities: self.options.source_capabilities.clone(),
            });
        }
        drop(runtime);
        if let Some(notifications) = self.options.extension_notifications.as_ref() {
            for warning in warnings {
                notifications.notify(warning, ExtensionNotifyType::Warning);
            }
        }
        for dispatch in dispatches {
            spawn_file_view_layout_dispatch(dispatch, sender.clone());
        }
        self.file_presentation_rendering
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .reconcile_active_layouts(&prepared);
        prepared
    }

    fn published_extension_line_highlights(&self) -> LineHighlightMap {
        let runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        merge_line_highlight_maps(
            runtime.line_highlight_preparation.resolved(),
            &self.agent_line_highlights,
        )
    }

    fn prepare_extension_line_highlights(
        &self,
        changeset: &Changeset,
        comments: &[ReviewComment],
    ) -> LineHighlightMap {
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let registrations = runtime.line_highlights.registrations().to_vec();
        let epochs = runtime.line_highlights.epochs().clone();
        if registrations.is_empty() {
            // Still retire old work and marks when the final provider disappears.
            runtime.line_highlight_preparation.reconcile(
                &[],
                &registrations,
                &epochs,
                std::iter::empty(),
            );
            return merge_line_highlight_maps(
                runtime.line_highlight_preparation.resolved(),
                &self.agent_line_highlights,
            );
        }
        let annotations =
            saved_extension_annotations(changeset, comments, self.options.agent_notes);
        let files = public_review::merge_file_annotations_borrowed(&changeset.files, &annotations);
        let extensions = runtime
            .extensions
            .iter()
            .cloned()
            .map(|extension| {
                Arc::new(line_highlights::SourceBoundLineHighlightRuntime {
                    runtime: Arc::new(extension),
                    sources: self.options.source_capabilities.clone(),
                }) as Arc<dyn LineHighlightRuntime>
            })
            .collect::<Vec<_>>();
        let filter_changed = runtime
            .line_highlight_preparation
            .set_stream_filter(&self.filter);
        let notes_changed = runtime
            .line_highlight_preparation
            .set_stream_notes(saved_extension_notes_identity(comments));
        let visibility_changed = runtime
            .line_highlight_preparation
            .set_stream_agent_notes(self.options.agent_notes);
        if filter_changed || notes_changed || visibility_changed {
            runtime.line_highlight_preparation.discard_file_results(
                files
                    .iter()
                    .map(std::borrow::Cow::as_ref)
                    .filter(|file| annotations.contains_key(public_review::public_file_id(file)))
                    .map(|file| file.runtime_id.as_str()),
            );
        }
        runtime.line_highlight_preparation.reconcile(
            &extensions,
            &registrations,
            &epochs,
            files
                .iter()
                .map(std::borrow::Cow::as_ref)
                .filter(|file| diff_file_matches_filter(file, &self.filter)),
        );
        merge_line_highlight_maps(
            runtime.line_highlight_preparation.resolved(),
            &self.agent_line_highlights,
        )
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
        self.scroll_to_selected_line();
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
        self.current_review_rows_with_options(&self.options, ReviewRowPurpose::Paint)
    }

    fn current_review_rows_with_options(
        &self,
        options: &ReviewOptions,
        purpose: ReviewRowPurpose,
    ) -> ReviewRows {
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
        let line_highlights = if matches!(purpose, ReviewRowPurpose::Geometry) {
            self.published_extension_line_highlights()
        } else {
            self.prepare_extension_line_highlights(state.changeset(), state.comments())
        };
        let mut highlights = self
            .highlights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let comments = comments_with_thread_draft(state.comments(), self.note_composer.as_ref());
        let mut rows = build_live_review_rows(
            state.changeset(),
            &comments,
            state.selection(),
            layout,
            options,
            width,
            &mut highlights,
            &self.expanded_gaps,
            &line_highlights,
            &file_view_layouts,
            &component_expanded,
            &self.file_presentation_rendering,
            self.options.extension_notifications.as_ref(),
            &self.filter,
            purpose,
        );
        if let Some(composer) = &self.note_composer {
            rows.insert_composer(
                composer,
                width,
                layout,
                &self.options.theme,
                state.changeset().files.get(composer.target.file_index),
            );
        }
        rows
    }

    fn live_copy_selection_snapshot(&self) -> Option<LiveCopySelectionSnapshot> {
        let rows = self.current_review_rows();
        let width = usize::from(self.review_width.get().max(1));
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let layout = state.resolved_layout(self.review_width.get());
        let visible_files = state
            .changeset()
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| diff_file_matches_filter(file, &self.filter))
            .collect::<Vec<_>>();
        if visible_files.is_empty() {
            return None;
        }

        let mut geometry_cache = DiffSectionGeometryCache::default();
        let section_geometry = visible_files
            .iter()
            .map(|(_, file)| {
                let mut options =
                    DiffSectionGeometryOptions::new(file, layout, &self.options.theme);
                options.show_hunk_headers = self.options.hunk_headers;
                options.width = width;
                options.show_line_numbers = self.options.line_numbers;
                options.line_number_digits = self.options.line_number_digits;
                options.wrap_lines = self.options.wrap_lines;
                options.reserve_add_note_column = false;
                options.tab_width = self.options.tab_width;
                options.hunk_gap = usize::from(self.options.hunk_gap);
                geometry_cache.measure(options)
            })
            .collect::<Vec<_>>();
        let files = visible_files
            .into_iter()
            .map(|(_, file)| file.clone())
            .collect::<Vec<_>>();
        drop(state);

        let body_heights = section_geometry
            .iter()
            .map(|geometry| i64::try_from(geometry.body_height).unwrap_or(i64::MAX))
            .collect::<Vec<_>>();
        let header_heights = build_in_stream_file_header_heights(&files);
        let file_section_layouts = build_file_section_layouts(
            &files,
            &body_heights,
            Some(&header_heights),
            i64::from(self.options.file_gap),
        );
        let header_stats_width = max_file_header_stats_width(&files);
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(2 + u16::from(!self.options.pager)),
        );
        let max_scroll = rows.lines.len().saturating_sub(viewport);
        let scroll = if self.scroll == usize::MAX {
            max_scroll
        } else {
            self.scroll.min(max_scroll)
        };
        let pinned_file_index = find_header_owning_file_section(
            &file_section_layouts,
            i64::try_from(scroll.saturating_sub(1)).unwrap_or(i64::MAX),
        )
        .map(|section| section.section_index);

        Some(LiveCopySelectionSnapshot {
            files,
            pinned_file_index,
            file_section_layouts,
            section_geometry,
            line_cursors: rows.line_cursors,
            code_horizontal_offset: self.options.horizontal_offset,
            copy_decorations: self.copy_decorations,
            header_label_width: width.saturating_sub(2 + header_stats_width + 1),
            header_stats_width,
            layout,
            reserve_add_note_column: false,
            scroll,
            show_hunk_headers: self.options.hunk_headers,
            show_line_numbers: self.options.line_numbers,
            width,
            wrap_lines: self.options.wrap_lines,
        })
    }

    fn resolve_live_copy_selection_point(
        &self,
        snapshot: &LiveCopySelectionSnapshot,
        event: &MouseEvent,
    ) -> Option<CopySelectionPoint> {
        let bounds = self.review_bounds.get()?;
        let content_top = bounds.y.saturating_add(2);
        let content_height = bounds
            .height
            .saturating_sub(2 + u16::from(!self.options.pager));
        if snapshot.copy_decorations
            && event.row == content_top.saturating_sub(1)
            && event.column >= bounds.x
            && event.column < bounds.right()
            && let Some(file) = snapshot
                .pinned_file_index
                .and_then(|index| snapshot.files.get(index))
        {
            return Some(CopySelectionPoint::PinnedHeader {
                column: usize::from(event.column.saturating_sub(bounds.x)),
                file_id: review_file_id(file).to_owned(),
                next_visual_row: i64::try_from(snapshot.scroll).ok()?,
            });
        }
        if event.column < bounds.x
            || event.column >= bounds.right()
            || event.row < content_top
            || event.row >= content_top.saturating_add(content_height)
        {
            return None;
        }
        let column = i64::from(event.column.saturating_sub(bounds.x));
        let viewport_row = usize::from(event.row.saturating_sub(content_top));
        let visual_row = i64::try_from(snapshot.scroll.saturating_add(viewport_row)).ok()?;
        find_copy_selection_point(
            column,
            snapshot.copy_decorations,
            &snapshot.file_section_layouts,
            &snapshot.section_geometry,
            visual_row,
            snapshot.width,
        )
    }

    fn live_copy_cursor_for_click(
        snapshot: &LiveCopySelectionSnapshot,
        point: &CopySelectionPoint,
        side: Option<CopySelectionSide>,
    ) -> Option<ReviewLineCursor> {
        let CopySelectionPoint::ReviewRow { visual_row, .. } = point else {
            return None;
        };
        let row = usize::try_from(*visual_row).ok()?;
        let mut candidates = snapshot
            .line_cursors
            .iter()
            .copied()
            .filter(|cursor| cursor.row == row);
        let first = candidates.next()?;
        let target_side = match side {
            Some(CopySelectionSide::Left) => Some(ReviewSide::Old),
            Some(CopySelectionSide::Right) => Some(ReviewSide::New),
            None => None,
        };
        target_side
            .and_then(|side| candidates.find(|cursor| cursor.target.side == side))
            .or_else(|| (target_side == Some(first.target.side)).then_some(first))
            .or(Some(first))
    }

    fn cancel_copy_selection(&mut self) {
        self.copy_selection_drag = None;
        self.copy_selection_snapshot = None;
    }

    fn reset_copy_click_sequence(&mut self) {
        self.last_copy_click_time = None;
        self.last_copy_click_point = None;
        self.copy_click_count = 0;
    }

    fn finish_copy_selection_text(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if self.clipboard_copy_supported {
            self.clipboard_copy_request = Some(text);
            self.status = Some("Copied selection to clipboard".into());
        } else {
            self.status = Some(
                "Clipboard copy unsupported in this terminal (enable OSC 52 to capture selections)"
                    .into(),
            );
        }
    }

    fn handle_copy_selection_mouse_at(&mut self, event: &MouseEvent, now: Instant) -> bool {
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self
                    .extension_pane_runtime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .file_view_component_hits
                    .iter()
                    .any(|hit| rect_contains(hit.bounds, event.column, event.row))
                {
                    return false;
                }
                let Some(snapshot) = self.live_copy_selection_snapshot() else {
                    self.cancel_copy_selection();
                    self.reset_copy_click_sequence();
                    return false;
                };
                let Some(point) = self.resolve_live_copy_selection_point(&snapshot, event) else {
                    self.cancel_copy_selection();
                    self.reset_copy_click_sequence();
                    return false;
                };
                let repeated_target = self.last_copy_click_point.as_ref().is_some_and(|previous| {
                    copy_selection_points_share_row(previous, &point)
                        && previous.column().abs_diff(point.column()) <= 2
                });
                let repeated_in_time = self.last_copy_click_time.is_some_and(|previous| {
                    now.saturating_duration_since(previous) < Duration::from_millis(350)
                });
                self.last_copy_click_time = Some(now);
                self.last_copy_click_point = Some(point.clone());
                self.copy_click_count = if repeated_target && repeated_in_time {
                    self.copy_click_count.saturating_add(1).min(3)
                } else {
                    1
                };

                let expanded = (self.copy_click_count >= 2)
                    .then(|| {
                        expand_selection_point(&point, self.copy_click_count, snapshot.context())
                    })
                    .flatten();
                let drag =
                    if let (Some(expanded), CopySelectionPoint::ReviewRow { visual_row, .. }) =
                        (expanded, &point)
                    {
                        CopySelectionDrag {
                            anchor: CopySelectionPoint::ReviewRow {
                                column: expanded.start_col,
                                visual_row: *visual_row,
                            },
                            focus: CopySelectionPoint::ReviewRow {
                                column: expanded.end_col,
                                visual_row: *visual_row,
                            },
                            moved: true,
                            expanded: true,
                        }
                    } else {
                        CopySelectionDrag {
                            anchor: point.clone(),
                            focus: point,
                            moved: false,
                            expanded: false,
                        }
                    };
                self.copy_selection_snapshot = Some(snapshot);
                self.copy_selection_drag = Some(drag);
                true
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(snapshot) = self.copy_selection_snapshot.as_ref() else {
                    return false;
                };
                let point = self.resolve_live_copy_selection_point(snapshot, event);
                let Some(drag) = self.copy_selection_drag.as_mut() else {
                    return false;
                };
                if let Some(point) = point {
                    drag.moved |= !copy_selection_points_equal(&point, &drag.anchor);
                    drag.focus = point;
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(mut drag) = self.copy_selection_drag.take() else {
                    return false;
                };
                let Some(snapshot) = self.copy_selection_snapshot.take() else {
                    return false;
                };
                if !drag.expanded
                    && let Some(point) = self.resolve_live_copy_selection_point(&snapshot, event)
                {
                    drag.moved |= !copy_selection_points_equal(&point, &drag.anchor);
                    drag.focus = point;
                }
                let side = resolve_copy_selection_side(
                    drag.anchor.column(),
                    snapshot.layout,
                    snapshot.width,
                );
                if copy_selection_drag_is_click(&drag) {
                    if self.options.cursor_line != CursorLineMode::Off
                        && let Some(cursor) =
                            Self::live_copy_cursor_for_click(&snapshot, &drag.anchor, side)
                    {
                        self.apply_review_line_cursor(cursor);
                        self.publish_extension_selection_events();
                        return true;
                    }
                    if !drag.moved {
                        return false;
                    }
                }
                let normalized = normalize_copy_selection_range(&drag.anchor, &drag.focus);
                let text = render_copy_selection_text(
                    snapshot.context(),
                    &normalized.start,
                    &normalized.end,
                    side,
                );
                self.finish_copy_selection_text(text);
                true
            }
            _ => false,
        }
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

    fn is_app_menu_toggle_key(key: &KeyEvent) -> bool {
        key.code == KeyCode::F(10)
            || key.code == KeyCode::Menu
            || (key.code == KeyCode::Char('e') && key.modifiers == KeyModifiers::ALT)
    }

    fn handle_app_menu_toggle_key(&mut self, key: &KeyEvent) -> bool {
        if !Self::is_app_menu_toggle_key(key) {
            return false;
        }
        let menus = self.app_menus();
        let target = if key.code == KeyCode::F(10) {
            MenuId::File
        } else if menus.contains_key(&MenuId::Extensions) {
            MenuId::Extensions
        } else {
            MenuId::File
        };
        self.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .menu
            .toggle(&menus, target);
        true
    }

    fn handle_app_menu_key(&mut self, key: &KeyEvent) -> bool {
        let menus = self.app_menus();
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

        // An advertised accelerator belongs to the open host menu before either
        // layer of extension-owned keyboard mode. This is especially important
        // while a focused file view and a session mode are active together: the
        // menu remains the host-owned route to Help and every other command.
        let live_key = to_live_extension_key_event(key);
        let commands = self.builtin_commands();
        if let Some(dispatch) = dispatch_app_command(&commands, &live_key) {
            if dispatch.closes_menu {
                self.close_app_menu();
            }
            let command_id = dispatch.command_id;
            self.apply_builtin_command_action(dispatch.action);
            self.publish_extension_lifecycle_event(ExtensionLifecycleEvent::CommandExecuted {
                command_id: command_id.into(),
            });
            return true;
        }
        if self.invoke_extension_command(key) {
            self.close_app_menu();
            return true;
        }

        // Unbound keys continue to the focused editor and layered extension modes.
        false
    }

    fn execute_app_menu_command(&mut self, command_id: &str) {
        if command_id == "workdeck.view.filePresentation.raw" {
            self.select_current_file_presentation_from_menu(None);
            return;
        }
        if let Some(view_key) = command_id.strip_prefix("workdeck.view.filePresentation.") {
            let registered = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .file_views
                .iter()
                .any(|registration| registered_file_view_key(&registration.view) == view_key);
            if registered {
                self.select_current_file_presentation_from_menu(Some(view_key));
            }
            return;
        }
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

    fn scroll_to_selected_line(&mut self) {
        let rows = self.current_review_geometry_rows();
        let Some(cursor) = self.current_review_line_cursor_in(&rows.line_cursors) else {
            self.scroll_to_selection();
            return;
        };
        self.apply_review_line_cursor(cursor);
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(2 + u16::from(!self.options.pager))
                .max(1),
        );
        let file_top = rows
            .file_body_tops
            .get(&cursor.target.file_index)
            .copied()
            .unwrap_or(0);
        self.scroll = cursor
            .row
            .saturating_sub(2_usize.max(viewport / 4))
            .max(file_top)
            .min(rows.lines.len().saturating_sub(viewport));
    }

    fn scroll_to_selection(&mut self) {
        self.scroll_to_reveal(ReviewRevealRequest {
            anchor: ReviewRevealAnchor::Hunk,
            scroll_to_note: false,
        });
    }

    fn scroll_to_reveal(&mut self, reveal: ReviewRevealRequest) {
        if reveal.anchor == ReviewRevealAnchor::None {
            return;
        }
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let selected = state.selection();
        let reveal_note_id = reveal.scroll_to_note.then(|| {
            let file = state.changeset().files.get(selected.file_index)?;
            let hunk_index = selected.hunk_index?;
            let candidates = state
                .comments()
                .iter()
                .filter(|comment| {
                    comment.resolution == ReviewNoteResolution::Active
                        && comment.anchor.file_key == file.key
                        && (comment.anchor.owner_hunk_index == Some(hunk_index)
                            || comment
                                .anchor
                                .intersecting_hunk_indices
                                .contains(&hunk_index))
                        && (self.options.agent_notes || comment.source == "user")
                })
                .map(|comment| ReviewRevealNoteCandidate {
                    id: comment.id.clone(),
                    line: comment
                        .line
                        .or(comment.anchor.preferred_line)
                        .or_else(|| comment.anchor.new_range.map(|range| range.start))
                        .or_else(|| comment.anchor.old_range.map(|range| range.start))
                        .unwrap_or(u32::MAX),
                    draft: false,
                })
                .collect::<Vec<_>>();
            resolve_review_reveal_note_id(&candidates)
        });
        let width = self.review_width.get();
        let layout = state.resolved_layout(width);
        let file_view_layouts = self.prepare_extension_file_view_layouts(state.changeset(), width);
        let component_expanded = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .file_view_component_expanded
            .clone();
        let line_highlights = self.published_extension_line_highlights();
        let mut highlights = self
            .highlights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Reveal consumes row geometry, not syntax styles. Painting still uses the
        // highlighted plan; keep the same layout/extension/note inputs here.
        let mut geometry_options = self.options.clone();
        geometry_options.highlight = false;
        let rows = build_live_review_rows(
            state.changeset(),
            state.comments(),
            selected,
            layout,
            &geometry_options,
            width,
            &mut highlights,
            &self.expanded_gaps,
            &line_highlights,
            &file_view_layouts,
            &component_expanded,
            &self.file_presentation_rendering,
            self.options.extension_notifications.as_ref(),
            &self.filter,
            ReviewRowPurpose::Geometry,
        );
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(2 + u16::from(!self.options.pager))
                .max(1),
        );
        let max_scroll = rows.lines.len().saturating_sub(viewport);
        let file_top = rows
            .file_tops
            .get(&selected.file_index)
            .copied()
            .unwrap_or(0);
        let file_body_top = rows
            .file_body_tops
            .get(&selected.file_index)
            .copied()
            .unwrap_or(file_top);
        self.scroll = match reveal.anchor {
            ReviewRevealAnchor::FileTop => file_body_top,
            ReviewRevealAnchor::Hunk => {
                let hunk_key = (selected.file_index, selected.hunk_index.unwrap_or(0));
                let (top, height) = reveal_note_id
                    .flatten()
                    .and_then(|note_id| rows.note_bounds.get(&note_id).copied())
                    .or_else(|| {
                        rows.hunk_tops.get(&hunk_key).copied().map(|top| {
                            (top, rows.hunk_heights.get(&hunk_key).copied().unwrap_or(1))
                        })
                    })
                    .unwrap_or((file_body_top, 1));
                let padding = 2_usize.max(viewport / 4);
                usize::try_from(compute_hunk_reveal_scroll_top(
                    i64::try_from(top).unwrap_or(i64::MAX),
                    i64::try_from(height).unwrap_or(i64::MAX),
                    i64::try_from(padding).unwrap_or(i64::MAX),
                    i64::try_from(viewport).unwrap_or(i64::MAX),
                ))
                .unwrap_or(usize::MAX)
                .max(file_body_top)
            }
            ReviewRevealAnchor::None => self.scroll,
        }
        .min(max_scroll);
    }

    fn navigate(&mut self, action: impl FnOnce(&mut ReviewState) -> bool) {
        if self.with_state(action) {
            self.reconcile_active_file_view_mode();
            self.scroll_to_selection();
            self.publish_extension_selection_events();
        }
    }

    fn reconcile_active_file_view_mode(&mut self) {
        let active = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_file_view_mode
            .clone();
        let Some(active) = active else {
            return;
        };
        let review_is_current = self.with_state(|state| {
            state.generation() == active.review_generation
                && state
                    .selected_file()
                    .is_some_and(|file| file.runtime_id == active.file.id)
        });
        let view_is_current = self
            .presented_extension_file_view(&active.file.id)
            .as_deref()
            == Some(active.view_key.as_str())
            && self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .file_views
                .iter()
                .any(|registration| {
                    registration.registration_identity == active.registration_identity
                });
        if !review_is_current || !view_is_current {
            self.exit_active_file_view_mode();
        }
    }

    fn toggle_source_gap(&mut self) {
        let target = self.with_state(|state| {
            let selection = state.selection();
            let file = state.selected_file()?;
            let source = review_gap_source_for_file(file);
            let hunk_index = selection.hunk_index.unwrap_or(0);
            let (gap_slot, address) = review_leading_gap(&source, hunk_index)
                .map(|gap| (hunk_index, gap))
                .or_else(|| {
                    review_trailing_gap(&source)
                        .filter(|gap| gap.hunk_index == hunk_index)
                        .map(|gap| (file.hunks.len(), gap))
                })?;
            Some((
                (file.key.clone(), gap_slot),
                selection.file_index,
                address,
                review_expansion_side(file.change_kind),
            ))
        });
        let Some((key, file_index, address, side)) = target else {
            self.status = Some("source expansion is unavailable for this file".into());
            return;
        };
        self.toggle_source_gap_target(key, file_index, address, side);
    }

    fn toggle_source_gap_for_file(&mut self, file_key: &str, gap_slot: usize) {
        let target = self.with_state(|state| {
            let (file_index, file) = state
                .changeset()
                .files
                .iter()
                .enumerate()
                .find(|(_, file)| file.key == file_key)?;
            let geometry = workdeck_review::review_gap_geometry_for_file(file);
            let address = if gap_slot == file.hunks.len() {
                geometry.trailing_gap()
            } else {
                geometry.leading_gap(gap_slot)
            }?;
            Some((
                (file.key.clone(), gap_slot),
                file_index,
                address,
                review_expansion_side(file.change_kind),
            ))
        });
        if let Some((key, file_index, address, side)) = target {
            self.toggle_source_gap_target(key, file_index, address, side);
        }
    }

    fn toggle_source_gap_target(
        &mut self,
        key: (String, usize),
        file_index: usize,
        address: ReviewGapAddress,
        side: ReviewSide,
    ) {
        let previous = self
            .current_review_line_cursor()
            .map(|cursor| cursor.target);
        let expanded = !self.expanded_gaps.remove(&key);
        let target = if expanded {
            if let Some(previous) = previous {
                let file_key = self.with_state(|state| {
                    state
                        .changeset()
                        .files
                        .get(previous.file_index)
                        .map(|file| file.key.clone())
                });
                if let Some(file_key) = file_key {
                    self.gap_cursor_restore.insert(
                        key.clone(),
                        GapCursorRestorePoint {
                            file_key,
                            target: previous,
                        },
                    );
                }
            }
            self.expanded_gaps.insert(key.clone());
            self.status = Some("source gap expanded".into());
            Some(ReviewNoteTarget {
                file_index,
                hunk_index: address.hunk_index,
                side,
                line: match side {
                    ReviewSide::Old => address.old_range.start,
                    ReviewSide::New => address.new_range.start,
                },
            })
        } else {
            self.status = Some("source gap collapsed".into());
            self.gap_cursor_restore
                .remove(&key)
                .map(|restore| restore.target)
        };
        self.pending_source_reveal = if expanded {
            target.and_then(|target| {
                self.with_state(|state| {
                    state.changeset().files.get(file_index).map(|file| {
                        source_controller::PendingSourceReveal {
                            runtime_id: file.runtime_id.clone(),
                            gap: key.clone(),
                            target,
                        }
                    })
                })
            })
        } else {
            None
        };
        self.start_source_load(&key.0, side);
        let rows = self.current_review_rows();
        let cursors = review_line_cursors(&rows);
        // A collapse restores the saved anchor only if it actually removed the
        // current cursor. Moving clear of the gap must survive closing it.
        let target = if expanded {
            target
        } else {
            previous
                .filter(|previous| cursors.iter().any(|cursor| cursor.target == *previous))
                .or(target)
        };
        if let Some(cursor) = cursors
            .into_iter()
            .find(|cursor| Some(cursor.target) == target)
        {
            self.pending_source_reveal = None;
            self.apply_review_line_cursor(cursor);
        } else {
            self.seed_current_line_cursor();
        }
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(2 + u16::from(!self.options.pager))
                .max(1),
        );
        self.keep_current_line_visible(viewport, rows.lines.len().saturating_sub(1));
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
        let now = Instant::now();
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
        if self.handle_view_preference_prompt_mouse(&event) {
            return;
        }
        if self.handle_extension_confirm_mouse(&event) {
            return;
        }
        if self.handle_extension_select_mouse(&event) {
            return;
        }
        if self.handle_extension_input_mouse(&event) {
            return;
        }
        if self.has_extension_dialog() {
            return;
        }
        if self.handle_theme_selector_mouse(&event, now) {
            return;
        }
        if self.show_agent_skill {
            if let Some(hits) = self.agent_skill_dialog_hits.get()
                && let MouseEventKind::Up(_) = event.kind
            {
                if hits
                    .close
                    .is_some_and(|close| rect_contains(close, event.column, event.row))
                    || !rect_contains(hits.frame, event.column, event.row)
                {
                    self.show_agent_skill = false;
                    self.agent_skill_dialog_hits.set(None);
                } else if hits.copy_supported
                    && rect_contains(hits.copy_button, event.column, event.row)
                {
                    self.clipboard_copy_request = Some(AGENT_SKILL_PROMPT.to_string());
                    self.status = Some("Copied agent skill prompt to clipboard".into());
                }
            }
            return;
        }
        if self.show_help {
            if let Some(hits) = self.help_dialog_hits.get() {
                match event.kind {
                    MouseEventKind::Up(_)
                        if hits
                            .close
                            .is_some_and(|close| rect_contains(close, event.column, event.row))
                            || !rect_contains(hits.frame, event.column, event.row) =>
                    {
                        self.show_help = false;
                        self.help_dialog_hits.set(None);
                    }
                    MouseEventKind::ScrollUp
                        if rect_contains(hits.content, event.column, event.row) =>
                    {
                        self.help_scroll = self.help_scroll.saturating_sub(1);
                    }
                    MouseEventKind::ScrollDown
                        if rect_contains(hits.content, event.column, event.row) =>
                    {
                        self.help_scroll = self.help_scroll.saturating_add(1).min(hits.max_scroll);
                    }
                    _ => {}
                }
            }
            return;
        }
        if self.handle_note_mouse(&event, now) {
            return;
        }
        if self.copy_selection_drag.is_some() && self.handle_copy_selection_mouse_at(&event, now) {
            return;
        }
        if self.handle_extension_mode_badge_mouse(&event)
            || self.handle_app_menu_mouse(&event)
            || self.handle_extension_pane_mouse(&event)
            || self.handle_review_scrollbar_mouse(&event, now)
            || self.handle_extension_file_view_mouse(&event)
            || self.handle_sidebar_mouse(&event)
        {
            return;
        }
        if self.handle_review_gap_mouse(&event)
            || self.handle_copy_selection_mouse_at(&event, now)
            || self.handle_review_file_header_mouse(&event)
            || self.handle_horizontal_mouse_scroll(&event)
        {
            return;
        }
        self.handle_mouse_at(event.kind, now);
    }

    fn clear_note_hover(&mut self) {
        self.note_hover_state.terminal_blur();
        self.note_hover = None;
        self.note_hover_hit.set(None);
    }

    fn handle_note_mouse(&mut self, event: &MouseEvent, now: Instant) -> bool {
        if self.note_composer.is_some() {
            let inside = self
                .note_composer_bounds
                .get()
                .is_some_and(|bounds| rect_contains(bounds, event.column, event.row));
            if matches!(
                event.kind,
                MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            ) {
                self.note_composer.as_mut().unwrap().focused = inside;
            }
            if !inside {
                return false;
            }
            if event.kind == MouseEventKind::Moved {
                self.saved_note_hover = None;
            }
            if event.kind == MouseEventKind::Up(MouseButton::Left) {
                let action = self
                    .note_composer_actions
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .iter()
                    .find(|(bounds, _)| rect_contains(*bounds, event.column, event.row))
                    .map(|(_, action)| *action);
                match action {
                    Some(AgentInlineNoteAction::Save) => self.save_note_composer(),
                    Some(AgentInlineNoteAction::Cancel) => {
                        self.handle_note_composer_key(&KeyEvent::new(
                            KeyCode::Esc,
                            KeyModifiers::NONE,
                        ));
                    }
                    _ => {}
                }
            }
            return true;
        }
        if matches!(
            event.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
        ) {
            let action = self
                .saved_note_actions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .find(|(bounds, _)| rect_contains(*bounds, event.column, event.row))
                .map(|(_, action)| *action);
            if let Some(action) = action {
                if event.kind == MouseEventKind::Up(MouseButton::Left) {
                    match action {
                        AgentInlineNoteAction::Edit => self.open_active_note_edit(),
                        AgentInlineNoteAction::Reply => self.open_active_note_reply(),
                        AgentInlineNoteAction::Delete => {
                            if let Some((comment, _)) = self.active_note_for_composer(true) {
                                self.with_state(|state| state.remove_comment(&comment.id));
                            }
                            self.saved_note_hover = None;
                        }
                        _ => {}
                    }
                }
                return true;
            }
        }
        if let Some((bounds, target)) = self.note_hover_hit.get()
            && rect_contains(bounds, event.column, event.row)
            && matches!(
                event.kind,
                MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            )
        {
            if event.kind == MouseEventKind::Up(MouseButton::Left) {
                let rows = self.current_review_rows();
                if let Some(cursor) = rows
                    .line_cursors
                    .iter()
                    .find(|cursor| cursor.target == target)
                    .copied()
                {
                    self.apply_review_line_cursor(cursor);
                }
                self.open_note_composer();
                // A pointer anchor must win even when keyboard cursor highlighting is off.
                if let Some(composer) = self.note_composer.as_mut() {
                    composer.target = target;
                }
                self.clear_note_hover();
            }
            return true;
        }
        if matches!(
            event.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp | MouseEventKind::Drag(_)
        ) {
            self.clear_note_hover();
            return false;
        }
        if event.kind != MouseEventKind::Moved {
            return false;
        }
        let Some(area) = self.review_bounds.get().filter(|area| {
            rect_contains(*area, event.column, event.row) && event.row >= area.y.saturating_add(2)
        }) else {
            self.saved_note_hover = None;
            self.clear_note_hover();
            return false;
        };
        let rows = self.current_review_rows();
        let visual_row = self
            .scroll
            .saturating_add(usize::from(event.row - area.y - 2));
        self.saved_note_hover = rows.note_bounds.iter().find_map(|(id, (top, height))| {
            (visual_row >= *top && visual_row < top.saturating_add(*height)).then(|| id.clone())
        });
        let target = rows
            .line_cursors
            .iter()
            .filter(|cursor| cursor.row == visual_row)
            .max_by_key(|cursor| cursor.target.side == ReviewSide::New)
            .map(|cursor| cursor.target);
        let Some(target) = target else {
            self.clear_note_hover();
            return false;
        };
        let key = format!("{}:{visual_row}", target.file_index);
        let affordances = std::collections::HashMap::from([(
            key.clone(),
            ActiveAddNoteAffordance {
                hunk_index: target.hunk_index,
                target: Some(CodeRowLineTarget {
                    side: target.side,
                    line: target.line as usize,
                }),
            },
        )]);
        self.note_hover_state.hover_row(
            &key,
            &affordances,
            now.saturating_duration_since(self.note_hover_epoch)
                .as_millis() as u64,
            true,
        );
        self.note_hover = Some((visual_row, target));
        false
    }

    fn handle_review_scrollbar_mouse(&mut self, event: &MouseEvent, now: Instant) -> bool {
        let map = self.review_scrollbar_hits.get();
        let relative_y = map.map(|map| i32::from(event.row) - i32::from(map.track.y));
        let relative_y = relative_y.map_or(0, |position| position as isize);

        let mut next_scroll = None;
        let handled = {
            let mut scrollbar = self
                .review_scrollbar
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if scrollbar.is_dragging() {
                match event.kind {
                    MouseEventKind::Drag(MouseButton::Left) => {
                        if let Some(map) = map {
                            next_scroll = scrollbar
                                .drag_to(relative_y, map.geometry, now)
                                .map(|scroll| scroll.round() as usize);
                        }
                        true
                    }
                    MouseEventKind::Up(MouseButton::Left) => scrollbar.end_drag(now),
                    _ => false,
                }
            } else if let Some(map) = map
                && rect_contains(map.track, event.column, event.row)
            {
                if event.kind == MouseEventKind::Down(MouseButton::Left) {
                    if map.geometry.thumb_contains(relative_y) {
                        scrollbar.begin_drag(relative_y, map.scroll_top, now);
                    } else {
                        next_scroll =
                            scrollbar.track_click(relative_y, map.geometry, map.scroll_top, now);
                    }
                }
                matches!(
                    event.kind,
                    MouseEventKind::Down(MouseButton::Left)
                        | MouseEventKind::Drag(MouseButton::Left)
                        | MouseEventKind::Up(MouseButton::Left)
                )
            } else {
                false
            }
        };
        if let Some(next_scroll) = next_scroll {
            self.scroll = next_scroll;
        }
        handled
    }

    fn handle_view_preference_prompt_mouse(&mut self, event: &MouseEvent) -> bool {
        if !self.view_preference_quit.save_config_prompt_open() {
            return false;
        }
        let map = self
            .view_preference_prompt_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(map) = map else {
            return true;
        };
        match event.kind {
            MouseEventKind::Moved => {
                self.view_preference_prompt_hovered_action_key =
                    dialog_action_at(&map, event.column, event.row)
                        .map(|hit| hit.key_label.clone());
            }
            MouseEventKind::Up(_) => {
                if let Some(hit) = dialog_action_at(&map, event.column, event.row) {
                    match hit.index {
                        0 => self.save_view_preferences_and_quit(Instant::now()),
                        1 => self.discard_view_preferences_and_quit(),
                        2 => self.never_ask_to_save_view_preferences_and_quit(Instant::now()),
                        _ => self.close_save_config_prompt(),
                    }
                } else if map
                    .modal
                    .close
                    .is_some_and(|close| rect_contains(close, event.column, event.row))
                    || !rect_contains(map.modal.frame, event.column, event.row)
                {
                    self.close_save_config_prompt();
                }
            }
            _ => {}
        }
        true
    }

    fn handle_theme_selector_mouse(&mut self, event: &MouseEvent, now: Instant) -> bool {
        if !self.themes.selector_open {
            return false;
        }
        let plan = self
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(plan) = plan else {
            return true;
        };
        match event.kind {
            MouseEventKind::Moved => {
                let hovered = theme_selector_item_at(&plan, event.column, event.row)
                    .map(|hit| (hit.index, hit.id.clone()));
                if hovered.as_ref().map(|(_, id)| id.as_str())
                    != self.theme_selector_hovered_item_id.as_deref()
                {
                    self.clear_theme_hover_preview();
                    if let Some((index, item_id)) = hovered {
                        self.theme_selector_hovered_item_id = Some(item_id.clone());
                        self.pending_theme_hover_preview = Some(PendingThemeHoverPreview {
                            index,
                            item_id,
                            deadline: now + Duration::from_millis(THEME_HOVER_PREVIEW_DELAY_MS),
                            item_count: plan.window.item_count,
                            selected_index: plan.window.selected_index,
                            visible_rows: plan.window.visible_rows,
                        });
                    }
                }
            }
            MouseEventKind::ScrollUp => self.scroll_theme_selector_window(-1),
            MouseEventKind::ScrollDown => self.scroll_theme_selector_window(1),
            MouseEventKind::Up(_) => {
                if let Some(hit) = theme_selector_item_at(&plan, event.column, event.row) {
                    let index = hit.index;
                    self.clear_theme_hover_preview();
                    self.accept_theme_selector_item(index);
                } else if plan
                    .modal
                    .close
                    .is_some_and(|close| rect_contains(close, event.column, event.row))
                    || !rect_contains(plan.modal.frame, event.column, event.row)
                {
                    self.close_theme_selector();
                }
            }
            _ => {}
        }
        true
    }

    fn tick_theme_hover_preview(&mut self, now: Instant) {
        let Some(pending) = self.pending_theme_hover_preview.as_ref() else {
            return;
        };
        if now < pending.deadline {
            return;
        }
        let pending = self
            .pending_theme_hover_preview
            .take()
            .expect("pending preview was checked");
        let plan = self
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let still_current = self.themes.selector_open
            && self.theme_selector_hovered_item_id.as_deref() == Some(pending.item_id.as_str())
            && plan.as_ref().is_some_and(|plan| {
                plan.window.item_count == pending.item_count
                    && plan.window.selected_index == pending.selected_index
                    && plan.window.visible_rows == pending.visible_rows
                    && plan
                        .item_hits
                        .iter()
                        .any(|hit| hit.index == pending.index && hit.id == pending.item_id)
            });
        if still_current {
            self.preview_theme_selector_item(pending.index);
        }
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
                    self.scroll_to_reveal(workdeck_review::REVIEW_FILE_JUMP_REVEAL);
                    self.publish_extension_selection_events();
                }
                true
            }
            _ => false,
        }
    }

    fn handle_review_gap_mouse(&mut self, event: &MouseEvent) -> bool {
        if !matches!(
            event.kind,
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(_)
        ) {
            return false;
        }
        let target = self
            .review_gap_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|hit| rect_contains(hit.bounds, event.column, event.row))
            .map(|hit| (hit.file_key.clone(), hit.gap_slot, hit.generation));
        let Some((file_key, gap_slot, generation)) = target else {
            return false;
        };
        if self.with_state(|state| state.generation()) != generation {
            return true;
        }
        if matches!(event.kind, MouseEventKind::Up(_)) {
            self.toggle_source_gap_for_file(&file_key, gap_slot);
            self.publish_extension_selection_events();
        } else {
            self.cancel_copy_selection();
            self.reset_copy_click_sequence();
        }
        true
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
        let max_scroll = if self.review_geometry_published.get() {
            let viewport = usize::from(
                self.review_height
                    .get()
                    .saturating_sub(2 + u16::from(!self.options.pager)),
            );
            self.current_review_content_height()
                .saturating_sub(viewport)
        } else {
            usize::MAX
        };
        self.scroll = self.scroll.min(max_scroll);
        if integer_scroll > 0 {
            self.scroll = self
                .scroll
                .saturating_add(integer_scroll.unsigned_abs())
                .min(max_scroll);
        } else if integer_scroll < 0 {
            self.scroll = self.scroll.saturating_sub(integer_scroll.unsigned_abs());
        }
        self.mouse_scroll_accumulator -= integer_scroll as f64;
    }

    fn current_review_geometry_rows(&self) -> ReviewRows {
        let mut options = self.options.clone();
        options.highlight = false;
        self.current_review_rows_with_options(&options, ReviewRowPurpose::Geometry)
    }

    fn current_review_content_height(&self) -> usize {
        if !self.options.wrap_lines
            && self.note_composer.is_none()
            && self.expanded_gaps.is_empty()
            && self.agent_line_highlights.is_empty()
        {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let runtime = self
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.comments().is_empty()
                && runtime.extensions.is_empty()
                && runtime.file_views.is_empty()
                && runtime.line_highlights.registrations().is_empty()
                && let Some(cached) = self
                    .review_plain_height
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .as_ref()
                && cached.matches(
                    &state,
                    &self.options,
                    self.review_width.get(),
                    &self.filter,
                    self.extension_registry_generation,
                )
            {
                return cached.height;
            }
        }
        self.current_review_geometry_rows().lines.len()
    }

    fn retire_interactive_authority(&mut self) {
        if self.interactive_authority_retired {
            return;
        }
        self.interactive_authority_retired = true;
        self.extension_runtime_bridge.retire_mount();
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

    fn dispose_highlight_worker(&mut self) {
        self.highlights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .dispose_worker();
    }
}

impl Drop for ReviewApp {
    fn drop(&mut self) {
        self.retire_interactive_authority();
        self.dispose_highlight_worker();
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

#[derive(Debug, Clone)]
struct PendingThemeHoverPreview {
    index: usize,
    item_id: String,
    deadline: Instant,
    item_count: usize,
    selected_index: usize,
    visible_rows: usize,
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

type ReviewReloader<'a> = dyn FnMut() -> Result<Changeset> + 'a;
type DynamicReviewReloader<'a> = dyn FnMut(
        &workdeck_core::CliInput,
        &Path,
        bool,
        &workdeck_vcs::VcsCatalog,
        &[LoadedExtension],
    ) -> Result<DynamicReviewLoad>
    + 'a;

pub fn run_review(changeset: Changeset, options: ReviewOptions) -> Result<()> {
    run_review_inner(changeset, options, Vec::new(), None, None, None, None)
}

pub fn run_review_with_extensions(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
) -> Result<()> {
    run_review_inner(changeset, options, extensions, None, None, None, None)
}

pub fn run_review_with_reload<F>(
    changeset: Changeset,
    options: ReviewOptions,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut() -> Result<Changeset>,
{
    run_review_inner(
        changeset,
        options,
        Vec::new(),
        None,
        None,
        Some(reload),
        None,
    )
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
    run_review_inner(
        changeset,
        options,
        extensions,
        None,
        None,
        Some(reload),
        None,
    )
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
        None,
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
        None,
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
        None,
    )
}

/// Run an interactive review whose host can load any validated replacement input.
pub fn run_review_with_dynamic_input_reload_with_signature<F>(
    changeset: Changeset,
    options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    watch_input: ReviewWatchInput,
    vcs_catalog: Option<workdeck_vcs::VcsCatalog>,
    reload: &mut F,
) -> Result<()>
where
    F: FnMut(
        &workdeck_core::CliInput,
        &Path,
        bool,
        &workdeck_vcs::VcsCatalog,
        &[LoadedExtension],
    ) -> Result<DynamicReviewLoad>,
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
        vcs_catalog,
        None,
        Some(reload),
    )
}

fn run_review_inner(
    changeset: Changeset,
    mut options: ReviewOptions,
    extensions: Vec<LoadedExtension>,
    watch_input: Option<(workdeck_core::CliInput, PathBuf, Option<String>)>,
    watch_vcs_catalog: Option<workdeck_vcs::VcsCatalog>,
    mut reloader: Option<&mut ReviewReloader<'_>>,
    mut dynamic_reloader: Option<&mut DynamicReviewReloader<'_>>,
) -> Result<()> {
    if let Some((input, _, _)) = &watch_input {
        options.review_input = Some(input.clone());
    }
    let app_host_reload_seed = watch_input
        .as_ref()
        .map(|(input, cwd, _)| (input.clone(), cwd.clone()));
    let app_host_repo = options.repo.clone();
    let mut session_broker = InteractiveSessionBroker::start(&changeset, &options)?;
    let _panic_hook = InteractiveTerminalPanicHook::install(true);
    let mut terminal = match InteractiveTerminalSession::enter(true) {
        Ok(terminal) => terminal,
        Err(error) => {
            session_broker.stop();
            return Err(error);
        }
    };
    let result = (|| {
        let session_repo = options
            .repo
            .clone()
            .map(Ok)
            .unwrap_or_else(std::env::current_dir)?;
        let mut app = ReviewApp::new_with_extensions_and_session(
            changeset,
            options,
            extensions,
            session_broker.producer(),
            Some(session_broker.client()),
        );
        let mut app_host_reload = app_host_reload_seed
            .map(|(input, cwd)| AppHostReloadCoordinator::new(input, cwd, app_host_repo.as_deref()))
            .transpose()
            .map_err(anyhow::Error::msg)?;
        let mut app_host = AppHostController::attach(app.session_broker_client());
        if let Err(error) = app_host.publish_snapshot(&app) {
            app.status = Some(format!("failed to publish session snapshot: {error}"));
        }
        app.set_clipboard_copy_supported(true);
        let mut watch_vcs_catalog =
            watch_vcs_catalog.unwrap_or_else(|| workdeck_vcs::bundled_vcs_catalog().clone());
        let mut watched_input = watch_input.filter(|_| app.options.watch).and_then(
            |(input, cwd, initial_signature)| {
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
            },
        );
        let session = default_discovery_directory()
            .map(|directory| {
                ReviewSessionServer::spawn(app.shared_state(), session_repo, directory)
            })
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
            &mut app_host,
            &mut app_host_reload,
            &mut dynamic_reloader,
            &mut watch_vcs_catalog,
        );
        drop(session);
        drop(watched_input);
        app_host.retire();
        app.retire_interactive_authority();
        session_broker.stop();
        app.dispose_highlight_worker();
        result
    })();
    session_broker.stop();
    drop(terminal);
    result
}

#[allow(clippy::too_many_arguments)]
fn run_loop(
    terminal: &mut InteractiveTerminalSession,
    app: &mut ReviewApp,
    session_stop: Option<&AtomicBool>,
    session_reload: Option<&AtomicBool>,
    reloader: &mut Option<&mut ReviewReloader<'_>>,
    watched_input: &mut Option<WatchedInputDriver>,
    app_host: &mut AppHostController,
    app_host_reload: &mut Option<AppHostReloadCoordinator>,
    dynamic_reloader: &mut Option<&mut DynamicReviewReloader<'_>>,
    watch_vcs_catalog: &mut workdeck_vcs::VcsCatalog,
) -> Result<()> {
    let mut next_reload = Instant::now() + Duration::from_millis(250);
    let job_control = JobControlSupport::default();
    while !app.should_quit && !session_stop.is_some_and(|stop| stop.load(Ordering::Relaxed)) {
        if terminal.host_disconnected() {
            break;
        }
        if app.process_external_quit_signal() {
            break;
        }
        let mut session_reload_handler =
            |app: &mut ReviewApp,
             next_input: &serde_json::Value,
             options: workdeck_session::ReloadSessionOptions| {
                let coordinator = app_host_reload.as_mut().ok_or_else(|| {
                    "This Workdeck review does not have a reloadable launch input.".to_owned()
                })?;
                let plan = coordinator.plan(next_input, options)?;
                let loader = dynamic_reloader.as_deref_mut().ok_or_else(|| {
                    "This Workdeck review input does not expose a dynamic reload loader.".to_owned()
                })?;
                let result = commit_dynamic_review_reload(
                    app,
                    coordinator,
                    plan,
                    loader,
                    watch_vcs_catalog,
                )?;
                replace_watched_input(
                    app,
                    watched_input,
                    coordinator.current_input().clone(),
                    coordinator.current_cwd().to_path_buf(),
                    watch_vcs_catalog,
                );
                Ok(result)
            };
        app_host.process_pending(app, &mut session_reload_handler);
        app.poll_extension_commands();
        app.poll_source_requests();
        app.tick_extension_notifications(Instant::now());
        if let Err(error) = app_host.publish_snapshot(app) {
            app.status = Some(format!("failed to publish session snapshot: {error}"));
        }
        let draw_result = interactive_runtime::synchronized_frame(&mut io::stdout(), |_| {
            terminal
                .terminal_mut()
                .draw(|frame| {
                    let area = frame.area();
                    render(area, frame.buffer_mut(), app);
                    let footer = Rect::new(
                        area.x,
                        area.bottom().saturating_sub(1),
                        area.width,
                        u16::from(area.height > 0),
                    );
                    if let Some(position) = app
                        .extension_pane_input_cursor_position()
                        .or_else(|| app.status_filter_cursor_position(footer))
                    {
                        frame.set_cursor_position(position);
                    }
                })
                .map(|_| ())
        });
        if let Err(error) = draw_result {
            if terminal.disconnected_during_io(&error) {
                return Ok(());
            }
            return Err(error.into());
        }
        let has_event = match event::poll(Duration::from_millis(100)) {
            Ok(ready) => ready,
            Err(error) if terminal.disconnected_during_io(&error) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if has_event {
            let event = match event::read() {
                Ok(event) => event,
                Err(error) if terminal.disconnected_during_io(&error) => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            match event {
                Event::Key(key) => {
                    match job_control.action(key, JobControlPlatform::current(), false) {
                        Some(JobControlAction::Interrupt) => app.should_quit = true,
                        Some(JobControlAction::Suspend) => {
                            terminal.suspend_foreground_process_group()?;
                        }
                        None => {
                            app.handle_key(key);
                            if let Some(request) = app.take_editor_request()
                                && let Some(message) = open_review_editor_in_crossterm(
                                    terminal.terminal_mut(),
                                    &request,
                                )
                            {
                                app.status = Some(message);
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => app.handle_mouse_event(mouse),
                Event::Paste(text) => app.handle_paste(&text),
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => {}
            }
            let can_reload_extensions = dynamic_reloader.is_some() && app_host_reload.is_some();
            if app.process_extension_trust_request(can_reload_extensions)
                && let (Some(loader), Some(coordinator)) =
                    (dynamic_reloader.as_deref_mut(), app_host_reload.as_mut())
            {
                match commit_current_dynamic_review_reload(
                    app,
                    coordinator,
                    loader,
                    watch_vcs_catalog,
                    SessionReloadReason::Manual,
                    true,
                ) {
                    Ok(_) => replace_watched_input(
                        app,
                        watched_input,
                        coordinator.current_input().clone(),
                        coordinator.current_cwd().to_path_buf(),
                        watch_vcs_catalog,
                    ),
                    Err(_) => {
                        app.status = Some(
                            "Failed to reload after trusting this repository's extensions.".into(),
                        );
                    }
                }
            }
            if let Some(text) = app.take_clipboard_copy_request()
                && let Err(error) = write_osc52_clipboard(&mut io::stdout(), &text)
            {
                app.report_clipboard_copy_failure(error);
                app.set_clipboard_copy_supported(false);
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
            let reason = reload_request_reason(manual_requested, session_requested);
            reload_current_review(
                app,
                reloader,
                dynamic_reloader,
                app_host_reload,
                watch_vcs_catalog,
                watched_input,
                manual_requested || session_requested,
                reason,
            );
        }
        let watch_refresh_completed = if let Some(driver) = watched_input.as_mut() {
            let outcome = if let (Some(loader), Some(coordinator)) =
                (dynamic_reloader.as_deref_mut(), app_host_reload.as_mut())
            {
                driver.poll(Instant::now(), &mut || {
                    commit_current_dynamic_review_reload(
                        app,
                        coordinator,
                        loader,
                        watch_vcs_catalog,
                        SessionReloadReason::Watch,
                        false,
                    )
                    .map(|_| ())
                    .map_err(anyhow::Error::msg)
                })
            } else {
                driver.poll(Instant::now(), &mut || {
                    let changeset = load_current_review_input(reloader)?;
                    apply_reloaded_changeset(app, changeset, SessionReloadReason::Watch);
                    Ok::<(), anyhow::Error>(())
                })
            };
            if outcome.reload_pending {
                app.notify_watch_reload_pending();
                app.status = Some("review reload pending".into());
            }
            if let Some(error) = outcome.errors.last() {
                app.status = Some(format!("auto-reload failed: {error}"));
            }
            outcome.refreshes > 0
        } else {
            false
        };
        if watch_refresh_completed {
            if let Some(coordinator) = app_host_reload.as_ref() {
                replace_watched_input(
                    app,
                    watched_input,
                    coordinator.current_input().clone(),
                    coordinator.current_cwd().to_path_buf(),
                    watch_vcs_catalog,
                );
            } else {
                watched_input.take();
                app.status = Some(
                    "auto-reload stopped because the refreshed input has no watch authority".into(),
                );
            }
        }
    }
    Ok(())
}

/// Execute one AppHost reload transaction after bounds validation. This is the
/// common commit gate for broker, manual, watch, editor-return, and workspace
/// refresh paths: content, extensions, producer, broker registration, mounted
/// input, catalog, and coordinator advance together or remain unchanged.
fn commit_dynamic_review_reload(
    app: &mut ReviewApp,
    coordinator: &mut AppHostReloadCoordinator,
    plan: AppHostReloadPlan,
    loader: &mut DynamicReviewReloader<'_>,
    watch_vcs_catalog: &mut workdeck_vcs::VcsCatalog,
) -> Result<workdeck_session::ReloadedSessionResult, String> {
    if app.shutdown_requested() {
        return Err("The Workdeck review is shutting down and cannot reload.".into());
    }
    let reload_extensions = coordinator.requires_extension_reload(&plan);
    let current_extensions = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .extensions
        .clone();
    let loaded = loader(
        &plan.input,
        &plan.cwd,
        reload_extensions,
        watch_vcs_catalog,
        &current_extensions,
    )
    .map_err(|error| format!("Failed to load session review input: {error:#}"))?;
    let mut committed_plan = plan;
    committed_plan.input = loaded.input.clone();
    let replacement_catalog = loaded.replacement_vcs_catalog.clone();
    let result = app.session_commit_dynamic_reload(loaded, &committed_plan.options)?;
    if let Some(catalog) = replacement_catalog {
        *watch_vcs_catalog = catalog;
    }
    coordinator.commit(&committed_plan);
    Ok(result)
}

fn commit_current_dynamic_review_reload(
    app: &mut ReviewApp,
    coordinator: &mut AppHostReloadCoordinator,
    loader: &mut DynamicReviewReloader<'_>,
    watch_vcs_catalog: &mut workdeck_vcs::VcsCatalog,
    reason: SessionReloadReason,
    reload_extensions: bool,
) -> Result<workdeck_session::ReloadedSessionResult, String> {
    let request = app.current_review_reload_request().ok_or_else(|| {
        "This Workdeck review does not have a reloadable launch input.".to_owned()
    })?;
    let next_input = serde_json::to_value(workdeck_session::core_cli_input_to_daemon(
        request.next_input,
    ))
    .map_err(|error| format!("Failed to encode the current review input: {error}"))?;
    let reason = match reason {
        SessionReloadReason::Watch => workdeck_session::SessionReloadReason::Watch,
        SessionReloadReason::Daemon => workdeck_session::SessionReloadReason::Daemon,
        SessionReloadReason::Manual => workdeck_session::SessionReloadReason::Manual,
    };
    let plan = coordinator.plan(
        &next_input,
        workdeck_session::ReloadSessionOptions {
            reset_app: Some(false),
            source_path: request.source_path,
            reason: Some(reason),
            reload_extensions: reload_extensions.then_some(true),
        },
    )?;
    commit_dynamic_review_reload(app, coordinator, plan, loader, watch_vcs_catalog)
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

#[allow(clippy::too_many_arguments)]
fn reload_current_review(
    app: &mut ReviewApp,
    reloader: &mut Option<&mut ReviewReloader<'_>>,
    dynamic_reloader: &mut Option<&mut DynamicReviewReloader<'_>>,
    app_host_reload: &mut Option<AppHostReloadCoordinator>,
    watch_vcs_catalog: &mut workdeck_vcs::VcsCatalog,
    watched_input: &mut Option<WatchedInputDriver>,
    report_unavailable: bool,
    reason: SessionReloadReason,
) {
    let can_reload =
        reloader.is_some() || (dynamic_reloader.is_some() && app_host_reload.is_some());
    if !can_reload {
        if report_unavailable {
            app.status = Some("this review input cannot be reloaded".into());
        }
        return;
    }
    if let (Some(loader), Some(coordinator)) =
        (dynamic_reloader.as_deref_mut(), app_host_reload.as_mut())
    {
        match commit_current_dynamic_review_reload(
            app,
            coordinator,
            loader,
            watch_vcs_catalog,
            reason,
            false,
        ) {
            Ok(_) => replace_watched_input(
                app,
                watched_input,
                coordinator.current_input().clone(),
                coordinator.current_cwd().to_path_buf(),
                watch_vcs_catalog,
            ),
            Err(error) => app.status = Some(format!("reload failed: {error}")),
        }
        return;
    }
    match load_current_review_input(reloader) {
        Ok(changeset) if !app.shutdown_requested() => {
            apply_reloaded_changeset(app, changeset, reason);
        }
        Ok(_) => {}
        Err(error) => app.status = Some(format!("reload failed: {error:#}")),
    }
}

fn load_current_review_input(reloader: &mut Option<&mut ReviewReloader<'_>>) -> Result<Changeset> {
    let Some(reload) = reloader.as_deref_mut() else {
        anyhow::bail!("this review input cannot be reloaded");
    };
    reload()
}

fn replace_watched_input(
    app: &mut ReviewApp,
    watched_input: &mut Option<WatchedInputDriver>,
    input: workdeck_core::CliInput,
    cwd: PathBuf,
    vcs_catalog: &workdeck_vcs::VcsCatalog,
) {
    let runtime: Arc<dyn WatchedInputRuntime> = Arc::new(NativeWatchedInputRuntime::new(
        cwd,
        Some(vcs_catalog.clone()),
    ));
    if let Err(error) = replace_watched_input_driver(
        watched_input,
        app.options.watch,
        input,
        runtime,
        None,
        Instant::now(),
        workdeck_vcs::WatchControllerConfig::default(),
    ) {
        app.status = Some(format!("failed to initialize watch mode: {error}"));
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
    let menu_bar_visible = app.show_menu_bar;
    let footer_visible = !app.options.pager;
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(u16::from(menu_bar_visible)),
            Constraint::Min(1),
            Constraint::Length(u16::from(footer_visible)),
        ])
        .split(area);
    render_app_menu_bar(outer[0], buffer, app);
    render_body(outer[1], buffer, app);
    render_footer(outer[2], buffer, app);
    render_app_menu_dropdown(area, buffer, app);
    if app.show_agent_skill {
        let map = render_agent_skill_dialog(
            area,
            buffer,
            &app.options.theme,
            app.clipboard_copy_supported,
        );
        app.agent_skill_dialog_hits.set(Some(AgentSkillDialogHits {
            frame: map.modal.frame,
            close: map.modal.close,
            copy_button: map.copy_button,
            copy_supported: map.copy_supported,
        }));
    } else {
        app.agent_skill_dialog_hits.set(None);
    }
    if app.show_help {
        let commands = app.help_commands();
        let map = render_help(area, buffer, &commands, &app.options.theme, app.help_scroll);
        app.help_dialog_hits.set(Some(HelpDialogHits {
            frame: map.modal.frame,
            close: map.modal.close,
            content: map.modal.content,
            max_scroll: map.max_scroll,
        }));
    } else {
        app.help_dialog_hits.set(None);
    }
    if app.themes.selector_open {
        let catalog = app.theme_catalog();
        let items = app.themes.items(&catalog);
        let selected_index = app.themes.selected_index(&catalog);
        let base_theme = resolve_theme(Some(&app.themes.active), None, &app.options.custom_themes);
        let plan = render_theme_selector_dialog(
            area,
            buffer,
            &items,
            selected_index,
            app.themes.window,
            &base_theme,
        );
        *app.theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(plan);
    } else {
        *app.theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
    render_note_composer(area, buffer, app);
    render_extension_input_dialog(area, buffer, app);
    render_extension_select_dialog(area, buffer, app);
    render_extension_confirm_dialog(area, buffer, app);
    render_extension_workspace_write_dialog(area, buffer, app);
    render_view_preference_save_prompt(area, buffer, app);
    render_extension_trust_prompt(area, buffer, app);
}

/// Draw the Hunk-compatible save-or-discard modal over a dirty review view.
pub fn render_view_preference_save_prompt(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    if !app.view_preference_quit.save_config_prompt_open() {
        *app.view_preference_prompt_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        return;
    }
    let current = app.current_view_preferences();
    let changes = app.view_preference_quit.changed_view_preferences(&current);
    let diff_lines = app
        .view_preference_quit
        .view_preference_diff_lines(&current);
    let actions = [
        ConfirmDialogAction::new("enter/s", "save"),
        ConfirmDialogAction::new("q", "discard"),
        ConfirmDialogAction::new("n", "never ask"),
        ConfirmDialogAction::new("esc", "cancel"),
    ];
    let body_row_count = 4_usize.saturating_add(diff_lines.len());
    let map = render_confirm_dialog(
        area,
        buffer,
        68,
        u16::try_from(confirm_dialog_height(body_row_count)).unwrap_or(u16::MAX),
        "Save view preferences?",
        true,
        body_row_count,
        &actions,
        app.view_preference_prompt_hovered_action_key.as_deref(),
        &app.options.theme,
    );
    let panel = ratatui_theme_color(&app.options.theme.panel);
    let muted = Style::default()
        .fg(ratatui_theme_color(&app.options.theme.muted))
        .bg(panel);
    let neutral = Style::default()
        .fg(ratatui_theme_color(&app.options.theme.badge_neutral))
        .bg(panel);
    let changed = changes.len();
    let body_width = usize::from(map.body.width);
    let rows = [
        (
            format!(
                "You changed {changed} view {} during this review.",
                if changed == 1 { "setting" } else { "settings" }
            ),
            muted,
        ),
        (
            format!(
                "Save {} to your config before quitting?",
                if changed == 1 { "it" } else { "them" }
            ),
            muted,
        ),
        (String::new(), muted),
        (
            app.view_preference_quit.view_preferences_config_label(),
            neutral,
        ),
    ];
    for (index, (text, style)) in rows.into_iter().enumerate() {
        let y = map
            .body
            .y
            .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        if y >= map.body.bottom() {
            break;
        }
        paint_confirm_dialog_text(
            buffer,
            Rect::new(map.body.x, y, map.body.width, 1),
            &fit_text(&text, body_width, None),
            style,
        );
    }
    for (index, line) in diff_lines.iter().enumerate() {
        let y = map
            .body
            .y
            .saturating_add(u16::try_from(index.saturating_add(4)).unwrap_or(u16::MAX));
        if y >= map.body.bottom() {
            break;
        }
        let color = if line.removed {
            &app.options.theme.badge_removed
        } else {
            &app.options.theme.badge_added
        };
        paint_confirm_dialog_text(
            buffer,
            Rect::new(map.body.x, y, map.body.width, 1),
            &fit_text(&line.text, body_width, None),
            Style::default().fg(ratatui_theme_color(color)).bg(panel),
        );
    }
    *app.view_preference_prompt_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(map);
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
        "Repository extensions: .agents/workdeck/extensions.",
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
        *app.extension_input_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        return;
    };
    let plan = render_extension_input_dialog_view(
        area,
        buffer,
        &dialog,
        app.extension_dialog_hovered_action_key.as_deref(),
        &app.options.theme,
    );
    *app.extension_input_dialog_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(plan);
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
        *app.extension_select_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        return;
    };
    let plan = render_extension_select_dialog_view(
        area,
        buffer,
        &dialog,
        app.extension_dialog_hovered_action_key.as_deref(),
        &app.options.theme,
    );
    *app.extension_select_dialog_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(plan);
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
        *app.extension_confirm_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        return;
    };
    let requested_width = 72_u16.min(40_u16.max(area.width.saturating_sub(8)));
    let maximum_frame = resolve_modal_geometry(requested_width, u16::MAX, area.width, area.height);
    let body_width = usize::from(maximum_frame.width.saturating_sub(4).max(1));
    let available_body_rows =
        usize::from(maximum_frame.height).saturating_sub(confirm_dialog_height(0));
    let attribution_rows = usize::from(dialog.show_attribution && available_body_rows > 0);
    let rows_after_attribution = available_body_rows.saturating_sub(attribution_rows);
    let attribution_gap_rows = usize::from(
        attribution_rows > 0 && !dialog.body_lines.is_empty() && rows_after_attribution > 1,
    );
    let body_rows = rows_after_attribution.saturating_sub(attribution_gap_rows);
    let visible_body = window_dialog_text(&dialog.body_lines, body_width, body_rows);
    let rendered_body_rows = visible_body
        .lines
        .len()
        .saturating_add(attribution_rows)
        .saturating_add(attribution_gap_rows);
    let actions = [
        ConfirmDialogAction::new("enter/y", &dialog.confirm_label),
        ConfirmDialogAction::new("esc/n", &dialog.cancel_label),
    ];
    let map = render_confirm_dialog(
        area,
        buffer,
        requested_width,
        u16::try_from(confirm_dialog_height(rendered_body_rows)).unwrap_or(u16::MAX),
        &dialog.title,
        true,
        rendered_body_rows,
        &actions,
        app.extension_confirm_hovered_action_key.as_deref(),
        &app.options.theme,
    );
    let mut row_index = 0_usize;
    if attribution_rows > 0 {
        let text = fit_text(
            &format!("{} {}", extension_toast_prefix(), dialog.extension_id),
            body_width,
            None,
        );
        paint_confirm_dialog_text(
            buffer,
            Rect::new(
                map.body.x,
                map.body.y,
                map.body.width,
                u16::from(map.body.height > 0),
            ),
            &text,
            Style::default()
                .fg(ratatui_theme_color(&app.options.theme.badge_neutral))
                .bg(ratatui_theme_color(&app.options.theme.panel)),
        );
        row_index = row_index.saturating_add(1 + attribution_gap_rows);
    }
    for line in visible_body.lines {
        let y = map
            .body
            .y
            .saturating_add(u16::try_from(row_index).unwrap_or(u16::MAX));
        if y >= map.body.bottom() {
            break;
        }
        paint_confirm_dialog_text(
            buffer,
            Rect::new(map.body.x, y, map.body.width, 1),
            &fit_text(&line, body_width, None),
            Style::default()
                .fg(ratatui_theme_color(&app.options.theme.muted))
                .bg(ratatui_theme_color(&app.options.theme.panel)),
        );
        row_index = row_index.saturating_add(1);
    }
    *app.extension_confirm_dialog_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(map);
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
    let body_padding = u16::from(!app.options.pager);
    let body_area = Rect::new(
        area.x.saturating_add(body_padding),
        area.y,
        area.width.saturating_sub(body_padding.saturating_mul(2)),
        area.height,
    );
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
    let pane_requests_current_line = static_specs.iter().any(|spec| spec.pane.current_line) || {
        let runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.panes.iter().any(|registration| {
            runtime.open.contains(&registration.key) && registration.pane.current_line
        })
    };
    let current_line_paint = pane_requests_current_line
        .then(|| app.current_extension_line_paint())
        .flatten();
    let current_line = current_line_paint
        .as_ref()
        .map(|paint| ExtensionCurrentLinePaintContext {
            side: match paint.side {
                ReviewSide::Old => ExtensionFileSide::Old,
                ReviewSide::New => ExtensionFileSide::New,
            },
            line: paint.line,
        });
    // Only open registered extension panes consume the full file projection
    // below. Built-in and static panes render directly from their existing models.
    let needs_visible_files = {
        let runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime
            .panes
            .iter()
            .any(|pane| runtime.open.contains(&pane.key))
    };
    let (
        generation,
        selection,
        has_changes,
        responsive_shows_sidebar,
        visible_files,
        selected_file_id,
        selected_hunk_index,
    ) = app.with_state(|state| {
        let selection = state.selection();
        let files = &state.changeset().files;
        let visible_files = files
            .iter()
            .enumerate()
            .filter(|_| needs_visible_files)
            .filter(|(_, file)| diff_file_matches_filter(file, &app.filter))
            .map(|(_, file)| project_extension_diff_file(file))
            .collect::<Vec<_>>();
        (
            state.generation(),
            selection,
            !files.is_empty(),
            state.responsive_layout(area.width).show_sidebar,
            visible_files,
            files
                .get(selection.file_index)
                .map(|file| file.runtime_id.clone()),
            selection.hunk_index,
        )
    });
    let mut keybindings = ExtensionResolvedKeybindings {
        keys: app.resolved_command_keys.keys.clone(),
    };
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for command in &runtime.app_commands {
        keybindings
            .keys
            .insert(command.id.clone(), command.keys.clone());
    }
    runtime.pane_action_hits.clear();
    let mut specs = static_specs.clone();
    specs.extend(runtime.session_panes.iter().map(ExtensionPaneSpec::from));
    let mut open = runtime.open.clone();
    open.retain(|key| {
        runtime
            .panes
            .iter()
            .find(|registration| registration.key == *key)
            .is_none_or(|registration| {
                !runtime
                    .failed_pane_registration_ids
                    .contains(&registration.registered.identity)
            })
    });
    let availability_candidates = runtime
        .panes
        .iter()
        .filter(|registration| open.contains(&registration.key) && registration.pane.available)
        .cloned()
        .collect::<Vec<_>>();
    let mut unavailable_keys = BTreeSet::new();
    for registration in availability_candidates {
        let pane_current_line = registration
            .pane
            .current_line
            .then_some(current_line)
            .flatten();
        let signature = PaneAvailabilitySignature {
            registration_identity: registration.registered.identity,
            files: visible_files.clone(),
            selected_file_id: selected_file_id.clone(),
            selected_hunk_index,
            current_line: pane_current_line,
        };
        let cached = runtime
            .cached_availability
            .get(&registration.registered.identity)
            .filter(|cached| cached.signature == signature)
            .map(|cached| cached.available);
        let available = if let Some(cached) = cached {
            Some(cached)
        } else if runtime.extensions[registration.extension_index].request_pending() {
            runtime
                .cached_availability
                .get(&registration.registered.identity)
                .map(|cached| cached.available)
                .or(Some(true))
        } else {
            match runtime.extensions[registration.extension_index].pane_available(
                PaneAvailabilityRequest {
                    pane_id: registration.pane.id.clone(),
                    placement: registration.pane.placement,
                    files: visible_files.clone(),
                    selected_file_id: selected_file_id.clone(),
                    selected_hunk_index,
                    current_line: pane_current_line,
                },
            ) {
                Ok(available) => {
                    runtime.cached_availability.insert(
                        registration.registered.identity,
                        CachedPaneAvailability {
                            signature,
                            available,
                        },
                    );
                    Some(available)
                }
                Err(error) => {
                    if let Some(warning) =
                        runtime.contain_pane_availability_failure(&registration, error)
                        && let Some(notifications) = app.options.extension_notifications.as_ref()
                    {
                        notifications.notify(warning, ExtensionNotifyType::Warning);
                    }
                    None
                }
            }
        };
        if available != Some(true) {
            unavailable_keys.insert(registration.key);
        }
    }
    open.retain(|key| !unavailable_keys.contains(key));
    let logical_files_replacement_open = runtime.session_panes.iter().any(|pane| {
        runtime.open.contains(&pane.key)
            && pane.registered.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY)
            && !runtime
                .failed_pane_registration_ids
                .contains(&pane.registered.identity)
    });
    let has_live_files_replacement = runtime.session_panes.iter().any(|pane| {
        pane.registered.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY)
            && !runtime
                .failed_pane_registration_ids
                .contains(&pane.registered.identity)
    });
    let open_files_replacement = runtime.session_panes.iter().any(|pane| {
        open.contains(&pane.key)
            && pane.registered.pane.replaces.as_deref() == Some(WORKDECK_FILES_PANE_KEY)
            && !runtime
                .failed_pane_registration_ids
                .contains(&pane.registered.identity)
    });
    if has_changes
        && (runtime.force_builtin_files_sidebar
            || (app.options.sidebar
                && !open_files_replacement
                && (!has_live_files_replacement || logical_files_replacement_open)))
    {
        open.insert(WORKDECK_FILES_PANE_KEY.into());
    }
    if app.options.sidebar_visibility == SidebarVisibility::Auto
        && !responsive_shows_sidebar
        && !runtime.force_builtin_files_sidebar
    {
        let side_keys = runtime
            .session_panes
            .iter()
            .filter(|pane| matches!(pane.placement, PanePlacement::Left | PanePlacement::Right))
            .map(|pane| pane.key.clone())
            .collect::<Vec<_>>();
        for key in side_keys {
            open.remove(&key);
        }
    }
    open.extend(static_specs.iter().map(|spec| spec.key.clone()));
    let plan = plan_extension_panes(
        &specs,
        &open,
        &runtime.size_overrides,
        body_area,
        48,
        MIN_EXTENSION_REVIEW_HEIGHT,
    );
    runtime.layout = plan.clone();

    let mut rendered_panes = Vec::new();
    let mut bundled_sidebar = None;
    let mut snapshot = None;
    for planned in &plan.panes {
        if planned.key == WORKDECK_FILES_PANE_KEY {
            bundled_sidebar = Some((planned.bounds, planned.divider));
            continue;
        }
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
                app.options.extension_panes[index]
                    .pane
                    .current_line
                    .then(|| current_line_paint.clone())
                    .flatten(),
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
            registration_identity: registration.registered.identity,
            generation,
            selection,
            files: visible_files.clone(),
            selected_file_id: selected_file_id.clone(),
            selected_hunk_index,
            current_line: registration
                .pane
                .current_line
                .then_some(current_line)
                .flatten(),
            keybindings: keybindings.clone(),
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
                files: signature.files.clone(),
                selected_file_id: signature.selected_file_id.clone(),
                selected_hunk_index: signature.selected_hunk_index,
                current_line: signature.current_line,
                keybindings: signature.keybindings.clone(),
            };
            let result = match runtime.extensions[registration.extension_index].render_pane(request)
            {
                Ok(result) => result,
                Err(error) => {
                    if let Some(failure) = runtime.contain_pane_render_failure(&registration, error)
                        && let Some(notifications) = app.options.extension_notifications.as_ref()
                    {
                        notifications.notify(failure.warning, ExtensionNotifyType::Warning);
                    }
                    continue;
                }
            };
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
                registration.registered.identity,
            )),
            registration
                .pane
                .current_line
                .then(|| current_line_paint.clone())
                .flatten(),
        ));
    }
    drop(runtime);

    render_builtin_body(plan.review_bounds, buffer, app);
    if let Some((sidebar_area, divider)) = bundled_sidebar {
        render_sidebar(sidebar_area, buffer, app);
        if let Some(divider) = divider {
            render_extension_pane_divider(
                divider,
                buffer,
                WORKDECK_FILES_PANE_KEY,
                PanePlacement::Left,
                app,
            );
        }
    } else {
        app.sidebar_bounds.set(None);
        app.sidebar_file_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
    let mut focused_pane_input = None;
    for (key, pane, pane_area, divider, owner, current_line_paint) in rendered_panes {
        if let Some(divider) = divider {
            render_extension_pane_divider(divider, buffer, &key, pane.pane.placement, app);
        }
        if focused_pane_input.is_none() {
            focused_pane_input = render_extension_pane(
                pane_area,
                buffer,
                &key,
                &pane,
                owner,
                current_line_paint.as_ref(),
                app,
            );
        } else {
            let _ = render_extension_pane(
                pane_area,
                buffer,
                &key,
                &pane,
                owner,
                current_line_paint.as_ref(),
                app,
            );
        }
    }
    let mut runtime = app
        .extension_pane_runtime
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    runtime.focused_pane_input = focused_pane_input.map(|candidate| {
        let retained_cursor = runtime.focused_pane_input.as_ref().and_then(|current| {
            (current.pane_key == candidate.pane_key
                && current.registration_identity == candidate.registration_identity
                && current.input_id == candidate.input_id
                && current.value == candidate.value)
                .then_some(current.cursor)
        });
        FocusedExtensionPaneInput {
            cursor: retained_cursor.unwrap_or_else(|| candidate.value.chars().count()),
            pane_key: candidate.pane_key,
            registration_identity: candidate.registration_identity,
            extension_index: candidate.extension_index,
            extension_id: candidate.extension_id,
            pane_id: candidate.pane_id,
            input_id: candidate.input_id,
            value: candidate.value,
            bounds: candidate.bounds,
            prefix_cells: candidate.prefix_cells,
        }
    });
}

fn pane_input_display(value: &str, placeholder: Option<&str>) -> String {
    if value.is_empty() {
        placeholder.unwrap_or_default().to_owned()
    } else {
        value.to_owned()
    }
}

fn render_builtin_body(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let state = app
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.changeset().is_empty() {
        app.sidebar_bounds.set(None);
        app.review_bounds.set(None);
        app.review_scrollbar_hits.set(None);
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
    drop(state);
    render_review(area, buffer, app);
}

fn render_extension_pane(
    area: Rect,
    buffer: &mut Buffer,
    pane_key: &str,
    pane: &ExtensionPaneView,
    owner: Option<(usize, String, String, u64)>,
    current_line_paint: Option<&ExtensionCurrentLinePaint>,
    app: &ReviewApp,
) -> Option<FocusedExtensionPaneInputCandidate> {
    let mut lines = Vec::new();
    let mut actions = Vec::new();
    let mut inputs = Vec::new();
    flatten_view(
        &pane.content,
        0,
        None,
        &mut lines,
        &mut actions,
        &mut inputs,
        current_line_paint,
    );
    let scroll_top = extension_pane_scroll_top(&pane.content, &lines, area.width, area.height);
    Block::default()
        .style(Style::default().bg(ratatui_theme_color(&app.options.theme.panel)))
        .render(area, buffer);
    let mut focused = None;
    if let Some((extension_index, extension_id, pane_id, registration_identity)) = owner {
        let mut physical_y = 0_usize;
        let viewport_start = scroll_top;
        let viewport_end = scroll_top.saturating_add(usize::from(area.height));
        let mut hits = Vec::new();
        for ((line, action_id), input) in lines.iter().zip(&actions).zip(&inputs) {
            let height = usize::from(extension_pane_line_height(line, area.width));
            let line_start = physical_y;
            let line_end = physical_y.saturating_add(height);
            let visible_start = line_start.max(viewport_start);
            let visible_end = line_end.min(viewport_end);
            let visible_height = visible_end.saturating_sub(visible_start);
            let y = area.y.saturating_add(
                u16::try_from(visible_start.saturating_sub(viewport_start)).unwrap_or(u16::MAX),
            );
            if let Some(action_id) = action_id
                && visible_height > 0
            {
                hits.push(ExtensionPaneActionHit {
                    bounds: Rect::new(
                        area.x,
                        y,
                        area.width,
                        u16::try_from(visible_height).unwrap_or(u16::MAX),
                    ),
                    extension_index,
                    extension_id: extension_id.clone(),
                    pane_id: pane_id.clone(),
                    action_id: action_id.clone(),
                });
            }
            if focused.is_none()
                && visible_height > 0
                && let Some(input) = input.as_ref().filter(|input| input.focused)
            {
                focused = Some(FocusedExtensionPaneInputCandidate {
                    pane_key: pane_key.to_owned(),
                    registration_identity,
                    extension_index,
                    extension_id: extension_id.clone(),
                    pane_id: pane_id.clone(),
                    input_id: input.input_id.clone(),
                    value: input.value.clone(),
                    bounds: Rect::new(
                        area.x,
                        y,
                        area.width,
                        u16::try_from(visible_height).unwrap_or(u16::MAX),
                    ),
                    prefix_cells: input.prefix_cells,
                });
            }
            physical_y = line_end;
        }
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pane_action_hits
            .extend(hits);
    }
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((u16::try_from(scroll_top).unwrap_or(u16::MAX), 0))
        .render(area, buffer);
    focused
}

fn extension_pane_selected_line_range(node: &ViewNode) -> Option<Range<usize>> {
    fn visit(node: &ViewNode, line: &mut usize, selected: &mut Option<Range<usize>>) {
        match node {
            ViewNode::Text { .. }
            | ViewNode::Row { .. }
            | ViewNode::Input { .. }
            | ViewNode::CurrentLine { .. }
            | ViewNode::Divider => *line = line.saturating_add(1),
            ViewNode::Column { children, gap } => {
                for (index, child) in children.iter().enumerate() {
                    if index > 0 {
                        *line = line.saturating_add(usize::from(*gap));
                    }
                    visit(child, line, selected);
                }
            }
            ViewNode::List {
                items,
                selected: selected_index,
            } => {
                for (index, item) in items.iter().enumerate() {
                    let start = *line;
                    *line = line.saturating_add(1);
                    visit(item, line, selected);
                    if selected.is_none() && *selected_index == Some(index) {
                        *selected = Some(start..*line);
                    }
                }
            }
            ViewNode::Action { child, .. } => visit(child, line, selected),
            ViewNode::Empty => {}
        }
    }

    let mut line = 0;
    let mut selected = None;
    visit(node, &mut line, &mut selected);
    selected
}

fn extension_pane_scroll_top(
    content: &ViewNode,
    lines: &[Line<'_>],
    width: u16,
    height: u16,
) -> usize {
    let Some(selected) = extension_pane_selected_line_range(content) else {
        return 0;
    };
    let heights = lines
        .iter()
        .map(|line| usize::from(extension_pane_line_height(line, width)))
        .collect::<Vec<_>>();
    let selected_start = heights.iter().take(selected.start).sum::<usize>();
    let selected_end = heights.iter().take(selected.end).sum::<usize>();
    let total = heights.iter().sum::<usize>();
    let viewport = usize::from(height);
    if selected_end <= viewport {
        0
    } else {
        selected_end
            .saturating_sub(viewport)
            .max(selected_start.saturating_sub(viewport.saturating_sub(1)))
            .min(total.saturating_sub(viewport))
    }
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
    inputs: &mut Vec<Option<FlattenedPaneInput>>,
    current_line_paint: Option<&ExtensionCurrentLinePaint>,
) {
    match node {
        ViewNode::Text { text, style } => {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(text.clone(), extension_style(style)),
            ]));
            actions.push(action_id.map(str::to_owned));
            inputs.push(None);
        }
        ViewNode::Row { children, gap } => {
            let mut spans = vec![Span::raw(" ".repeat(indent))];
            let mut row_input: Option<FlattenedPaneInput> = None;
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    spans.push(Span::raw(" ".repeat(usize::from(*gap))));
                }
                match child {
                    ViewNode::Text { text, style } => {
                        spans.push(Span::styled(text.clone(), extension_style(style)));
                    }
                    ViewNode::Input {
                        id,
                        value,
                        placeholder,
                        focused,
                    } => {
                        let prefix_cells =
                            u16::try_from(spans.iter().map(|span| span.width()).sum::<usize>())
                                .unwrap_or(u16::MAX);
                        spans.push(Span::styled(
                            pane_input_display(value, placeholder.as_deref()),
                            Style::default().add_modifier(Modifier::UNDERLINED),
                        ));
                        let candidate = FlattenedPaneInput {
                            input_id: id.clone(),
                            value: value.clone(),
                            focused: *focused,
                            prefix_cells,
                        };
                        if row_input
                            .as_ref()
                            .is_none_or(|current| !current.focused && candidate.focused)
                        {
                            row_input = Some(candidate);
                        }
                    }
                    ViewNode::CurrentLine { side, width } => {
                        spans
                            .extend(current_line_pane_row(current_line_paint, *side, *width).spans);
                    }
                    _ => spans.push(Span::raw("…")),
                }
            }
            lines.push(Line::from(spans));
            actions.push(action_id.map(str::to_owned));
            inputs.push(row_input);
        }
        ViewNode::Column { children, gap } => {
            for (index, child) in children.iter().enumerate() {
                if index > 0 {
                    for _ in 0..*gap {
                        lines.push(Line::default());
                        actions.push(action_id.map(str::to_owned));
                        inputs.push(None);
                    }
                }
                flatten_view(
                    child,
                    indent,
                    action_id,
                    lines,
                    actions,
                    inputs,
                    current_line_paint,
                );
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
                inputs.push(None);
                flatten_view(
                    item,
                    indent + 2,
                    action_id,
                    lines,
                    actions,
                    inputs,
                    current_line_paint,
                );
            }
        }
        ViewNode::Action { id, child } => {
            flatten_view(
                child,
                indent,
                Some(id),
                lines,
                actions,
                inputs,
                current_line_paint,
            );
        }
        ViewNode::Input {
            id,
            value,
            placeholder,
            focused,
        } => {
            lines.push(Line::from(vec![
                Span::raw(" ".repeat(indent)),
                Span::styled(
                    pane_input_display(value, placeholder.as_deref()),
                    Style::default().add_modifier(Modifier::UNDERLINED),
                ),
            ]));
            actions.push(action_id.map(str::to_owned));
            inputs.push(Some(FlattenedPaneInput {
                input_id: id.clone(),
                value: value.clone(),
                focused: *focused,
                prefix_cells: u16::try_from(indent).unwrap_or(u16::MAX),
            }));
        }
        ViewNode::CurrentLine { side, width } => {
            let mut line = current_line_pane_row(current_line_paint, *side, *width);
            if indent > 0 {
                line.spans.insert(0, Span::raw(" ".repeat(indent)));
            }
            lines.push(line);
            actions.push(action_id.map(str::to_owned));
            inputs.push(None);
        }
        ViewNode::Divider => {
            lines.push(Line::styled(
                format!("{}────────", " ".repeat(indent)),
                Style::default().fg(Color::DarkGray),
            ));
            actions.push(action_id.map(str::to_owned));
            inputs.push(None);
        }
        ViewNode::Empty => {}
    }
}

fn current_line_pane_row(
    paint: Option<&ExtensionCurrentLinePaint>,
    side: ExtensionFileSide,
    width: u16,
) -> Line<'static> {
    let Some(paint) = paint else {
        return Line::default();
    };
    let rendered = paint.render(
        match side {
            ExtensionFileSide::Old => ReviewSide::Old,
            ExtensionFileSide::New => ReviewSide::New,
        },
        usize::from(width),
    );
    paint_diff_row(DiffRowViewOptions {
        planned_row: None,
        row: Some(&rendered.row),
        width: rendered.width,
        line_number_digits: rendered.line_number_digits,
        show_line_numbers: rendered.show_line_numbers,
        show_hunk_headers: rendered.show_hunk_headers,
        wrap_lines: rendered.wrap_lines,
        code_horizontal_offset: rendered.code_horizontal_offset,
        theme: &rendered.theme,
        selected: rendered.selected,
        copy_selected_row_range: None,
        copy_selected_side: None,
        cursor_highlight: None,
        line_highlights: None,
        anchor_id: None,
        note_guide_side: None,
        show_add_note_badge: false,
        interactions: DiffRowInteractionIdentity::default(),
    })
    .and_then(|row| row.lines().first().map(PaintedCodeCellLine::ratatui_line))
    .unwrap_or_default()
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
        ViewNode::Input {
            value, placeholder, ..
        } => lines.push(Line::from(vec![
            Span::raw(" ".repeat(indent)),
            Span::styled(
                pane_input_display(value, placeholder.as_deref()),
                Style::default()
                    .fg(ratatui_theme_color(&theme.text))
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ])),
        ViewNode::CurrentLine { .. } => lines.push(Line::raw("…")),
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
    let mut block = Block::default();
    if app.show_menu_bar && !app.options.pager {
        block = block
            .title(format!(" {} ", bundled_files_pane().title))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(ratatui_theme_color(&app.options.theme.border)));
    }
    let mut inner = block.inner(area);
    // The public sidebar rows reserve one host-owned highlight lane; their own
    // one-column padding then leaves the same `pane width - 2` text geometry as Hunk.
    inner.width = inner.width.saturating_sub(1);
    block.render(area, buffer);
    app.sidebar_bounds.set(Some(inner));

    let mode = resolve_file_sidebar_mode(inner.width.saturating_sub(1));
    let visible_files = files
        .iter()
        .filter(|file| diff_file_matches_filter(file, &app.filter));
    let entries = match mode {
        FileSidebarMode::Flat => public_review::build_flat_sidebar_entries_borrowed(visible_files),
        FileSidebarMode::Tree => public_review::build_tree_sidebar_entries_borrowed(visible_files),
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

    let map = public_review::render_file_nav_entries(
        inner,
        buffer,
        entries,
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
    app.review_gap_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    let render_now = Instant::now();
    app.review_width.set(area.width);
    app.review_height.set(area.height);
    app.review_bounds.set(Some(area));
    app.review_geometry_published.set(true);
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
    let line_highlights =
        app.prepare_extension_line_highlights(state.changeset(), state.comments());
    let mut highlights = app
        .highlights
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let comments = comments_with_thread_draft(state.comments(), app.note_composer.as_ref());
    let viewport = area
        .height
        .saturating_sub(2 + u16::from(!app.options.pager)) as usize;
    // Only plain, unwrapped split streams use viewport painting for now. Complex
    // content retains the complete painter, including extension lifecycle calls.
    let highlight_files;
    let gap_geometries;
    let purpose = if layout == LayoutMode::Split
        && !app.options.wrap_lines
        && comments.is_empty()
        && app.note_composer.is_none()
        && app.expanded_gaps.is_empty()
        && file_view_layouts.is_empty()
        && line_highlights.is_empty()
        && state
            .changeset()
            .files
            .iter()
            .all(|file| file.agent.is_none())
    {
        let cached = app
            .review_plain_height
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .filter(|cached| {
                cached.matches(
                    &state,
                    &app.options,
                    area.width,
                    &app.filter,
                    app.extension_registry_generation,
                )
            })
            .map(|cached| {
                (
                    cached.height,
                    Arc::clone(&cached.sections),
                    Arc::clone(&cached.gap_geometries),
                )
            });
        let (content_height, layouts, cached_gaps) = cached.unwrap_or_else(|| {
            let mut geometry_options = app.options.clone();
            geometry_options.highlight = false;
            let geometry = build_live_review_rows(
                state.changeset(),
                &comments,
                state.selection(),
                layout,
                &geometry_options,
                area.width,
                &mut highlights,
                &app.expanded_gaps,
                &line_highlights,
                &file_view_layouts,
                &component_expanded,
                &app.file_presentation_rendering,
                app.options.extension_notifications.as_ref(),
                &app.filter,
                ReviewRowPurpose::Geometry,
            );
            let layouts = geometry
                .visible_file_indices
                .iter()
                .enumerate()
                .map(|(position, index)| {
                    let bottom = geometry
                        .visible_file_indices
                        .get(position + 1)
                        .map_or(geometry.lines.len(), |next| geometry.file_tops[next]);
                    FileSectionLayout {
                        file_id: index.to_string(),
                        section_index: position,
                        section_top: geometry.file_tops[index] as i64,
                        header_top: geometry.file_header_tops[index] as i64,
                        body_top: geometry.file_body_tops[index] as i64,
                        body_height: bottom.saturating_sub(geometry.file_body_tops[index]) as i64,
                        section_bottom: bottom as i64,
                    }
                })
                .collect::<Vec<_>>();
            let layouts = Arc::new(layouts);
            let gaps = Arc::new(
                state
                    .changeset()
                    .files
                    .iter()
                    .map(PlainFileGeometry::new)
                    .collect::<Vec<_>>(),
            );
            *app.review_plain_height
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(PlainReviewHeight {
                document: state.changeset_snapshot(),
                layout,
                width: area.width,
                filter: app.filter.clone(),
                file_gap: app.options.file_gap,
                hunk_gap: app.options.hunk_gap,
                hunk_headers: app.options.hunk_headers,
                pager: app.options.pager,
                registry_generation: app.extension_registry_generation,
                height: geometry.lines.len(),
                sections: Arc::clone(&layouts),
                gap_geometries: Arc::clone(&gaps),
            });
            (geometry.lines.len(), layouts, gaps)
        });
        gap_geometries = cached_gaps;
        let start = app.scroll.min(content_height.saturating_sub(viewport));
        let rapid = app
            .review_prefetch
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .observe(start as i64, viewport as i64, false, render_now);
        let ids = layouts
            .iter()
            .map(|section| section.file_id.as_str())
            .collect::<Vec<_>>();
        let selected = state.selection().file_index.to_string();
        let adjacent = adjacent_highlight_prefetch_ids(&ids, Some(&selected));
        highlight_files = highlight_prefetch_ids(
            &adjacent,
            &layouts,
            rapid,
            start as i64,
            viewport as i64,
            Some(&selected),
        )
        .into_iter()
        .filter_map(|id| id.parse::<usize>().ok())
        .collect::<BTreeSet<_>>();
        ReviewRowPurpose::Viewport {
            start,
            end: start.saturating_add(viewport),
            highlight_files: Some(&highlight_files),
            gap_geometries: Some(&gap_geometries),
            row_capacity: content_height,
        }
    } else {
        *app.review_plain_height
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        ReviewRowPurpose::Paint
    };
    let mut rows = build_live_review_rows(
        state.changeset(),
        &comments,
        state.selection(),
        layout,
        &app.options,
        area.width,
        &mut highlights,
        &app.expanded_gaps,
        &line_highlights,
        &file_view_layouts,
        &component_expanded,
        &app.file_presentation_rendering,
        app.options.extension_notifications.as_ref(),
        &app.filter,
        purpose,
    );
    if let Some(composer) = &app.note_composer {
        rows.insert_composer(
            composer,
            area.width,
            layout,
            &app.options.theme,
            state.changeset().files.get(composer.target.file_index),
        );
    }
    if rows.lines.is_empty() {
        rows.lines
            .extend(std::iter::repeat_with(Line::default).take(viewport.saturating_sub(1) / 2));
        rows.lines.push(
            Line::styled(
                "No files match the current filter.",
                Style::default().fg(ratatui_theme_color(&app.options.theme.muted)),
            )
            .alignment(Alignment::Center),
        );
    }
    let max_scroll = rows.lines.len().saturating_sub(viewport);
    let scroll = if app.scroll == usize::MAX {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    let content_height = rows.lines.len();
    app.review_prefetch
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .observe(
            scroll as i64,
            viewport as i64,
            app.options.wrap_lines,
            render_now,
        );
    let pinned_file_index = rows
        .visible_file_indices
        .iter()
        .copied()
        .rev()
        .find(|index| {
            rows.file_header_tops
                .get(index)
                .is_some_and(|top| *top <= scroll.saturating_sub(1))
        })
        .or_else(|| rows.visible_file_indices.first().copied());
    let pinned_header = pinned_file_index.and_then(|index| {
        state.changeset().files.get(index).map(|file| {
            file_header(
                file,
                usize::from(area.width),
                max_file_header_stats_width(&state.changeset().files),
                &app.options.theme,
            )
        })
    });
    let mut note_actions = app
        .saved_note_actions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    note_actions.clear();
    if app.note_composer.is_none()
        && let Some(id) = &app.saved_note_hover
        && let Some((top, height)) = rows.note_bounds.get(id)
        && let Some(comment) = state.comments().iter().find(|comment| &comment.id == id)
        && let Some(file) = state
            .changeset()
            .files
            .iter()
            .find(|file| file.key == comment.anchor.file_key)
    {
        let painted = paint_saved_comment(
            comment,
            state.comments(),
            file,
            layout,
            &app.options.theme,
            area.width,
            true,
        );
        for (offset, line) in painted
            .ratatui_lines()
            .into_iter()
            .take(*height)
            .enumerate()
        {
            rows.lines[*top + offset] = line;
        }
        for hit in painted.action_hits {
            let row = top.saturating_add(hit.row);
            if row >= scroll && row < scroll.saturating_add(viewport) {
                note_actions.push((
                    Rect::new(
                        area.x + hit.column_start as u16,
                        area.y + 2 + (row - scroll) as u16,
                        hit.width as u16,
                        1,
                    ),
                    hit.action,
                ));
            }
        }
    }
    drop(note_actions);
    *app.review_gap_hits
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = rows
        .gap_rows
        .iter()
        .filter(|(top, _, _)| {
            *top >= scroll && *top < scroll.saturating_add(viewport) && area.width > 0
        })
        .map(|(top, file_index, gap_slot)| ReviewGapMouseHit {
            bounds: Rect::new(
                area.x,
                area.y
                    .saturating_add(2)
                    .saturating_add(u16::try_from(top.saturating_sub(scroll)).unwrap_or(u16::MAX)),
                area.width,
                1,
            ),
            file_key: state.changeset().files[*file_index].key.clone(),
            gap_slot: *gap_slot,
            generation: state.generation(),
        })
        .collect();
    drop(state);
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
                    area.y.saturating_add(2).saturating_add(
                        u16::try_from(top.saturating_sub(scroll)).unwrap_or(u16::MAX),
                    ),
                    area.width,
                    1,
                ),
                file_index: *file_index,
            })
        })
        .collect();
    if let Some(file_index) = pinned_file_index {
        app.review_file_header_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(SidebarFileHit {
                bounds: Rect::new(
                    area.x,
                    area.y.saturating_add(1),
                    area.width,
                    area.height.saturating_sub(1).min(1),
                ),
                file_index,
            });
    }
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
                    area.y.saturating_add(2).saturating_add(
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
    let mut visible = visible;
    if let Some(header) = pinned_header {
        visible.insert(0, header);
    }
    Paragraph::new(visible)
        .block(
            Block::default()
                .title(" Review ")
                .borders(Borders::TOP)
                .border_style(border_style),
        )
        .render(area, buffer);
    paint_live_copy_selection(area, buffer, app);
    app.note_hover_hit.set(None);
    if app.note_composer.is_none()
        && let Some((row, target)) = app.note_hover
        && row >= scroll
        && row < scroll.saturating_add(viewport)
    {
        let width = CODE_ROW_ADD_NOTE_BADGE_WIDTH.min(area.width);
        let bounds = Rect::new(
            area.right().saturating_sub(width),
            area.y
                .saturating_add(2)
                .saturating_add((row - scroll) as u16),
            width,
            1,
        );
        if bounds.y < area.bottom() {
            Paragraph::new(CODE_ROW_ADD_NOTE_BADGE_TEXT)
                .style(
                    Style::default()
                        .fg(ratatui_theme_color(&app.options.theme.note_title_text))
                        .bg(ratatui_theme_color(
                            &app.options.theme.note_title_background,
                        )),
                )
                .render(bounds, buffer);
            app.note_hover_hit.set(Some((bounds, target)));
        }
    }
    let presentation = {
        let mut scrollbar = app
            .review_scrollbar
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        scrollbar.observe_scroll_top(scroll, Instant::now());
        scrollbar.presentation(content_height, viewport, scroll)
    };
    app.review_scrollbar_hits
        .set(presentation.and_then(|presentation| {
            render_vertical_review_scrollbar(area, buffer, presentation, &app.options.theme)
        }));
}

fn copy_selection_row_is_selectable(snapshot: &LiveCopySelectionSnapshot, visual_row: i64) -> bool {
    snapshot.file_section_layouts.iter().any(|section| {
        if visual_row < section.body_top
            || visual_row >= section.body_top.saturating_add(section.body_height)
        {
            return false;
        }
        let Some(geometry) = snapshot.section_geometry.get(section.section_index) else {
            return false;
        };
        let body_row = visual_row.saturating_sub(section.body_top);
        geometry.row_bounds.iter().any(|bounds| {
            let top = i64::try_from(bounds.bounds.top).unwrap_or(i64::MAX);
            let bottom =
                top.saturating_add(i64::try_from(bounds.bounds.height).unwrap_or(i64::MAX));
            body_row >= top && body_row < bottom
        })
    })
}

fn copy_selection_background(color: Color, theme: &AppTheme) -> Color {
    let base = match color {
        Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        Color::Reset => TRANSPARENT_BACKGROUND.to_owned(),
        _ => theme.panel.clone(),
    };
    ratatui_theme_color(&selection_highlight_background(&base, theme))
}

fn paint_live_copy_selection(area: Rect, buffer: &mut Buffer, app: &ReviewApp) {
    let Some(drag) = app.copy_selection_drag.as_ref().filter(|drag| drag.moved) else {
        return;
    };
    let Some(snapshot) = app.copy_selection_snapshot.as_ref() else {
        return;
    };
    let normalized = normalize_copy_selection_range(&drag.anchor, &drag.focus);
    let (
        CopySelectionPoint::ReviewRow {
            column: start_column,
            visual_row: start_row,
        },
        CopySelectionPoint::ReviewRow {
            column: end_column,
            visual_row: end_row,
        },
    ) = (&normalized.start, &normalized.end)
    else {
        return;
    };
    let side = resolve_copy_selection_side(drag.anchor.column(), snapshot.layout, snapshot.width);
    let split = (snapshot.layout == LayoutMode::Split)
        .then(|| resolve_diff_split_pane_widths(snapshot.width));
    let content_top = area.y.saturating_add(2);
    let viewport_height = area
        .height
        .saturating_sub(2 + u16::from(!app.options.pager));
    for viewport_row in 0..viewport_height {
        let visual_row = i64::try_from(snapshot.scroll.saturating_add(usize::from(viewport_row)))
            .unwrap_or(i64::MAX);
        if visual_row < *start_row
            || visual_row > *end_row
            || !copy_selection_row_is_selectable(snapshot, visual_row)
        {
            continue;
        }
        let mut first = if visual_row == *start_row {
            *start_column
        } else {
            0
        };
        let mut last = if visual_row == *end_row {
            *end_column
        } else {
            snapshot.width.saturating_sub(1)
        };
        if let Some(panes) = split {
            match side {
                Some(CopySelectionSide::Left) => {
                    last = last.min(panes.left_width.saturating_sub(1));
                }
                Some(CopySelectionSide::Right) => {
                    first = first.max(panes.left_width);
                }
                None => {}
            }
        }
        last = last.min(snapshot.width.saturating_sub(1));
        if first > last {
            continue;
        }
        let y = content_top.saturating_add(viewport_row);
        for column in first..=last {
            let Ok(column) = u16::try_from(column) else {
                break;
            };
            let x = area.x.saturating_add(column);
            if x >= area.right() {
                break;
            }
            let cell = &mut buffer[(x, y)];
            cell.set_bg(copy_selection_background(cell.bg, &app.options.theme));
        }
    }
}

fn render_vertical_review_scrollbar(
    review_area: Rect,
    buffer: &mut Buffer,
    presentation: VerticalScrollbarPresentation,
    theme: &AppTheme,
) -> Option<VerticalScrollbarRenderMap> {
    let track_height = u16::try_from(presentation.geometry.track_height)
        .unwrap_or(u16::MAX)
        .min(review_area.height.saturating_sub(2));
    if review_area.width == 0 || track_height == 0 {
        return None;
    }
    let track = Rect::new(
        review_area.right().saturating_sub(VERTICAL_SCROLLBAR_WIDTH),
        review_area.y.saturating_add(2),
        VERTICAL_SCROLLBAR_WIDTH.min(review_area.width),
        track_height,
    );
    Block::default()
        .style(Style::default().bg(ratatui_theme_color(&theme.border)))
        .render(track, buffer);

    let logical_top = i64::from(track.y)
        .saturating_add(i64::try_from(presentation.geometry.thumb_y).unwrap_or(i64::MAX));
    let logical_bottom = logical_top
        .saturating_add(i64::try_from(presentation.geometry.thumb_height).unwrap_or(i64::MAX));
    let clipped_top = logical_top.max(i64::from(track.y));
    let clipped_bottom = logical_bottom.min(i64::from(track.bottom()));
    let thumb = Rect::new(
        track.x,
        u16::try_from(clipped_top).unwrap_or(track.y),
        track.width,
        u16::try_from(clipped_bottom.saturating_sub(clipped_top)).unwrap_or(track.height),
    );
    if thumb.height > 0 {
        let color = if presentation.dragging {
            &theme.accent
        } else {
            &theme.accent_muted
        };
        Block::default()
            .style(Style::default().bg(ratatui_theme_color(color)))
            .render(thumb, buffer);
    }
    Some(VerticalScrollbarRenderMap {
        track,
        thumb,
        geometry: presentation.geometry,
        scroll_top: presentation.scroll_top,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct PlainFileGeometry {
    gaps: workdeck_review::ReviewGapGeometry,
    split_pairs: Vec<Vec<workdeck_diff::SplitLinePair>>,
}

impl PlainFileGeometry {
    fn new(file: &DiffFile) -> Self {
        Self {
            gaps: workdeck_review::review_gap_geometry_for_file(file),
            split_pairs: file
                .hunks
                .iter()
                .map(|hunk| plan_split_line_pairs(&hunk.lines))
                .collect(),
        }
    }
}

#[derive(Debug)]
struct PlainReviewHeight {
    document: Arc<Changeset>,
    layout: LayoutMode,
    width: u16,
    filter: String,
    file_gap: u16,
    hunk_gap: u16,
    hunk_headers: bool,
    pager: bool,
    registry_generation: u64,
    height: usize,
    sections: Arc<Vec<FileSectionLayout>>,
    gap_geometries: Arc<Vec<PlainFileGeometry>>,
}

impl PlainReviewHeight {
    fn matches(
        &self,
        state: &ReviewState,
        options: &ReviewOptions,
        width: u16,
        filter: &str,
        registry_generation: u64,
    ) -> bool {
        // The retained Arc prevents address reuse; document contents cannot mutate.
        std::ptr::eq(self.document.as_ref(), state.changeset())
            && self.layout == state.resolved_layout(width)
            && self.width == width
            && self.filter == filter
            && self.file_gap == options.file_gap
            && self.hunk_gap == options.hunk_gap
            && self.hunk_headers == options.hunk_headers
            && self.pager == options.pager
            && self.registry_generation == registry_generation
    }
}

#[derive(Debug)]
struct ReviewGapMouseHit {
    bounds: Rect,
    file_key: String,
    gap_slot: usize,
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GapCursorRestorePoint {
    file_key: String,
    target: ReviewNoteTarget,
}

/// Compact ordered row lookup. Duplicate rows retain the last inserted target,
/// matching the previous BTreeMap representation, including after composer shifts.
#[derive(Debug, Default, PartialEq, Eq)]
struct ReviewNoteTargets(Vec<(usize, ReviewNoteTarget)>);

impl ReviewNoteTargets {
    fn get(&self, row: &usize) -> Option<&ReviewNoteTarget> {
        self.0
            .binary_search_by_key(row, |(row, _)| *row)
            .ok()
            .map(|index| &self.0[index].1)
    }

    fn iter(&self) -> impl Iterator<Item = (&usize, &ReviewNoteTarget)> {
        self.0.iter().map(|(row, target)| (row, target))
    }

    #[cfg(test)]
    fn values(&self) -> impl Iterator<Item = &ReviewNoteTarget> {
        self.0.iter().map(|(_, target)| target)
    }
}

impl FromIterator<(usize, ReviewNoteTarget)> for ReviewNoteTargets {
    fn from_iter<T: IntoIterator<Item = (usize, ReviewNoteTarget)>>(entries: T) -> Self {
        let mut entries = entries.into_iter().collect::<Vec<_>>();
        if !entries.windows(2).all(|pair| pair[0].0 <= pair[1].0) {
            // Stable sorting preserves insertion order among duplicate rows.
            entries.sort_by_key(|(row, _)| *row);
        }
        entries.dedup_by(|later, earlier| {
            if later.0 == earlier.0 {
                earlier.1 = later.1;
                true
            } else {
                false
            }
        });
        Self(entries)
    }
}

impl IntoIterator for ReviewNoteTargets {
    type Item = (usize, ReviewNoteTarget);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[derive(Debug)]
struct ReviewRows {
    lines: Vec<Line<'static>>,
    note_targets: ReviewNoteTargets,
    note_bounds: std::collections::HashMap<String, (usize, usize)>,
    line_cursors: Vec<ReviewLineCursor>,
    file_tops: BTreeMap<usize, usize>,
    file_header_tops: BTreeMap<usize, usize>,
    file_body_tops: BTreeMap<usize, usize>,
    visible_file_indices: Vec<usize>,
    file_header_rows: Vec<(usize, usize)>,
    /// Logical row, owning file index, and gap slot; projected only after final layout.
    gap_rows: Vec<(usize, usize, usize)>,
    hunk_tops: std::collections::HashMap<(usize, usize), usize>,
    hunk_heights: std::collections::HashMap<(usize, usize), usize>,
    file_view_component_hits: Vec<FileViewComponentLogicalHit>,
}

fn paint_note_composer(
    composer: &ReviewNoteComposer,
    width: u16,
    layout: LayoutMode,
    theme: &AppTheme,
    file: Option<&DiffFile>,
) -> PaintedAgentInlineNote {
    let range = workdeck_core::LineRange {
        start: composer.target.line,
        end: composer.target.line,
    };
    let annotation = AgentAnnotation {
        extra: Default::default(),
        id: Some(composer.id.clone()),
        old_range: (composer.target.side == ReviewSide::Old).then_some(range),
        new_range: (composer.target.side == ReviewSide::New).then_some(range),
        summary: composer.body.clone(),
        rationale: None,
        markup: None,
        tags: Vec::new(),
        confidence: None,
        source: Some("user-draft".into()),
        title: match &composer.kind {
            ReviewNoteComposerKind::Create => None,
            ReviewNoteComposerKind::Edit { .. } => Some("Edit note".into()),
            ReviewNoteComposerKind::Reply { .. } => Some("Reply".into()),
        },
        author: None,
        created_at: None,
        updated_at: None,
        editable: true,
    };
    let mut options =
        AgentInlineNoteViewOptions::new(&annotation, layout, theme, usize::from(width));
    options.anchor_side = Some(composer.target.side);
    options.file = file;
    options.thread = composer.thread.as_ref();
    options.draft = Some(AgentInlineNoteDraft {
        body: &composer.body,
        focused: composer.focused,
        notify_focus: false,
        notify_blur: false,
    });
    paint_agent_inline_note(&AgentInlineNoteViewState::default(), options)
}

impl ReviewRows {
    fn insert_composer(
        &mut self,
        composer: &ReviewNoteComposer,
        width: u16,
        layout: LayoutMode,
        theme: &AppTheme,
        file: Option<&DiffFile>,
    ) {
        let Some(anchor) = self
            .note_targets
            .iter()
            .filter(|(_, target)| **target == composer.target)
            .map(|(row, _)| *row)
            .chain(
                self.line_cursors
                    .iter()
                    .filter(|cursor| cursor.target == composer.target)
                    .map(|cursor| cursor.row),
            )
            .max()
        else {
            return;
        };
        let (start, removed) = match &composer.kind {
            ReviewNoteComposerKind::Edit { target_note_id, .. } => self
                .note_bounds
                .remove(target_note_id)
                .unwrap_or((anchor.saturating_add(1), 0)),
            _ => (anchor.saturating_add(1), 0),
        };
        let painted = paint_note_composer(composer, width, layout, theme, file);
        let lines = painted.ratatui_lines();
        let height = lines.len();
        self.lines
            .splice(start..start.saturating_add(removed), lines);
        let shift = |row: &mut usize| {
            if *row >= start {
                *row = row
                    .saturating_sub(removed)
                    .saturating_add(height)
                    .max(start);
            }
        };
        self.note_targets = std::mem::take(&mut self.note_targets)
            .into_iter()
            .map(|(mut row, target)| {
                shift(&mut row);
                (row, target)
            })
            .collect();
        for cursor in &mut self.line_cursors {
            shift(&mut cursor.row);
        }
        for top in self
            .file_tops
            .values_mut()
            .chain(self.file_header_tops.values_mut())
            .chain(self.file_body_tops.values_mut())
        {
            shift(top);
        }
        for (_, top) in &mut self.file_header_rows {
            shift(top);
        }
        for (top, _, _) in &mut self.gap_rows {
            shift(top);
        }
        for top in self.hunk_tops.values_mut() {
            shift(top);
        }
        if let Some(hunk_height) = self
            .hunk_heights
            .get_mut(&(composer.target.file_index, composer.target.hunk_index))
        {
            *hunk_height = hunk_height.saturating_sub(removed).saturating_add(height);
        }
        for (top, _) in self.note_bounds.values_mut() {
            shift(top);
        }
        for hit in &mut self.file_view_component_hits {
            shift(&mut hit.top);
        }
        self.note_bounds
            .insert(composer.id.clone(), (start, height));
    }
}

#[derive(Debug)]
struct LiveCopySelectionSnapshot {
    files: Vec<DiffFile>,
    pinned_file_index: Option<usize>,
    file_section_layouts: Vec<FileSectionLayout>,
    section_geometry: Vec<Arc<DiffSectionGeometry>>,
    line_cursors: Vec<ReviewLineCursor>,
    code_horizontal_offset: usize,
    copy_decorations: bool,
    header_label_width: usize,
    header_stats_width: usize,
    layout: LayoutMode,
    reserve_add_note_column: bool,
    scroll: usize,
    show_hunk_headers: bool,
    show_line_numbers: bool,
    width: usize,
    wrap_lines: bool,
}

impl LiveCopySelectionSnapshot {
    fn context(&self) -> CopySelectionContext<'_> {
        CopySelectionContext {
            code_horizontal_offset: self.code_horizontal_offset,
            copy_decorations: self.copy_decorations,
            files: &self.files,
            file_section_layouts: &self.file_section_layouts,
            header_label_width: self.header_label_width,
            header_stats_width: self.header_stats_width,
            layout: self.layout,
            pinned_header_file: self
                .pinned_file_index
                .and_then(|index| self.files.get(index)),
            reserve_add_note_column: self.reserve_add_note_column,
            section_geometry: &self.section_geometry,
            show_hunk_headers: self.show_hunk_headers,
            show_line_numbers: self.show_line_numbers,
            width: self.width,
            wrap_lines: self.wrap_lines,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReviewLineCursor {
    row: usize,
    target: ReviewNoteTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReviewNoteTarget {
    file_index: usize,
    hunk_index: usize,
    side: ReviewSide,
    line: u32,
}

/// Collapse physical rows that paint the same source line into Hunk's navigable line stops.
/// Wrapped continuations and inline note rows retain the source target for hit-testing, but a
/// single `j`/`k` press must cross the source line exactly once.
fn review_line_cursors(rows: &ReviewRows) -> Vec<ReviewLineCursor> {
    rows.line_cursors.clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewNoteComposer {
    focused: bool,
    thread: Option<VisibleAgentNoteThread>,
    id: String,
    kind: ReviewNoteComposerKind,
    target: ReviewNoteTarget,
    body: String,
    cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReviewNoteComposerKind {
    Create,
    Edit {
        target_note_id: String,
        parent_id: Option<String>,
    },
    Reply {
        parent_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AgentSkillDialogHits {
    frame: Rect,
    close: Option<Rect>,
    copy_button: Rect,
    copy_supported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HelpDialogHits {
    frame: Rect,
    close: Option<Rect>,
    content: Rect,
    max_scroll: usize,
}

#[derive(Debug)]
struct TargetedHunkRows {
    lines: Vec<Line<'static>>,
    targets: Vec<Option<ReviewNoteTarget>>,
    cursor_targets: Vec<(usize, ReviewNoteTarget)>,
    note_bounds: Vec<(String, usize, usize)>,
}

#[derive(Debug)]
struct RenderedCommentRows {
    lines: Vec<Line<'static>>,
    note_bounds: Vec<(String, usize, usize)>,
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
        None,
        None,
        None,
        ReviewRowPurpose::Paint,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReviewRowPurpose<'a> {
    Paint,
    Geometry,
    Viewport {
        start: usize,
        end: usize,
        highlight_files: Option<&'a BTreeSet<usize>>,
        gap_geometries: Option<&'a [PlainFileGeometry]>,
        row_capacity: usize,
    },
}

fn diff_file_matches_filter(file: &DiffFile, filter: &str) -> bool {
    review_file_fields_match_filter(
        &file.path,
        file.previous_path.as_deref(),
        file.agent
            .as_ref()
            .and_then(|agent| agent.summary.as_deref()),
        filter,
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
    file_view_layouts: &BTreeMap<String, ResolvedFileViewLayout>,
    component_expanded: &BTreeSet<FileViewComponentStateKey>,
    file_presentation_rendering: &Mutex<FilePresentationRenderingController>,
    extension_notifications: Option<&ExtensionNotificationHub>,
    filter: &str,
    purpose: ReviewRowPurpose,
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
        Some(file_presentation_rendering),
        extension_notifications,
        Some(filter),
        purpose,
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
    file_view_layouts: &BTreeMap<String, ResolvedFileViewLayout>,
    component_expanded: &BTreeSet<FileViewComponentStateKey>,
    file_presentation_rendering: Option<&Mutex<FilePresentationRenderingController>>,
    extension_notifications: Option<&ExtensionNotificationHub>,
    filter: Option<&str>,
    purpose: ReviewRowPurpose,
) -> ReviewRows {
    let capacity = match purpose {
        ReviewRowPurpose::Viewport { row_capacity, .. } => row_capacity,
        _ => 0,
    };
    let mut rows = Vec::with_capacity(capacity);
    let mut file_tops = BTreeMap::new();
    let mut file_header_tops = BTreeMap::new();
    let mut file_body_tops = BTreeMap::new();
    let mut visible_file_indices = Vec::new();
    let mut file_header_rows = Vec::with_capacity(changeset.files.len());
    let mut gap_rows = Vec::new();
    let mut hunk_tops = std::collections::HashMap::new();
    let mut hunk_heights = std::collections::HashMap::new();
    let mut file_view_component_hits = Vec::new();
    // Rows arrive in stream order. Collect once into the ordered lookup instead
    // of rebalancing a B-tree for every source/code row in every frame.
    let mut note_targets = Vec::with_capacity(capacity);
    let mut note_bounds = std::collections::HashMap::new();
    let mut line_cursors = Vec::with_capacity(capacity);
    let visible = |_file_index: usize, file: &DiffFile| {
        filter.is_none_or(|filter| diff_file_matches_filter(file, filter))
    };
    let header_stats_width = changeset
        .files
        .iter()
        .enumerate()
        .filter(|(file_index, file)| visible(*file_index, file))
        .map(|(_, file)| file_header_stats(file).width)
        .max()
        .unwrap_or_default();
    let mut visible_file_position = 0_usize;
    // All file-local settings are read-only except the line-number width. Clone
    // the configuration once, then derive that override from the original options
    // for every file (never inherit the preceding file's digit count).
    let mut file_options = options.clone();
    for (file_index, file) in changeset.files.iter().enumerate() {
        if !visible(file_index, file) {
            continue;
        }
        file_options.line_number_digits = Some(
            options
                .line_number_digits
                .unwrap_or_else(|| find_max_line_number(file).to_string().len()),
        );
        let options = &file_options;
        let first_visible_file = visible_file_position == 0;
        visible_file_position = visible_file_position.saturating_add(1);
        visible_file_indices.push(file_index);
        file_tops.insert(file_index, rows.len());
        let has_file_view = file_view_layouts.contains_key(&file.runtime_id);
        let section = plan_diff_section(DiffSectionViewOptions {
            file_id: if file.runtime_id.is_empty() {
                &file.key
            } else {
                &file.runtime_id
            },
            has_file_view,
            // Native file-view geometry is measured immediately before painting.
            has_section_geometry: has_file_view,
            separator_width: usize::from(width.saturating_sub(2)),
            show_header: chrome.show_file_headers && (!live || !first_visible_file),
            separator_height: if first_visible_file {
                0
            } else {
                usize::from(options.file_gap)
            },
            theme: &options.theme,
        });
        if let Some(separator) = section.separator {
            rows.extend(separator.lines());
        }
        file_header_tops.insert(file_index, rows.len());
        if section.show_header {
            file_header_rows.push((file_index, rows.len()));
            rows.push(file_header(
                file,
                usize::from(width),
                header_stats_width,
                &options.theme,
            ));
        }
        file_body_tops.insert(file_index, rows.len());
        let file_selection = if selection.file_index == file_index {
            selection
        } else {
            ReviewSelection::default()
        };
        if options.agent_notes {
            rows.extend(agent_rows(file, layout, usize::from(width)));
        }
        if matches!(section.body, DiffSectionBodyRoute::FileView { .. })
            && let Some(resolved) = file_view_layouts.get(&file.runtime_id)
            && append_extension_file_view_rows(
                &mut rows,
                &mut hunk_tops,
                &mut hunk_heights,
                &mut note_bounds,
                file,
                file_index,
                file_selection,
                comments,
                resolved,
                options,
                usize::from(width),
                component_expanded,
                &mut file_view_component_hits,
                &mut line_cursors,
                file_presentation_rendering,
                extension_notifications,
            )
        {
            continue;
        }
        let should_highlight = !matches!(purpose,
            ReviewRowPurpose::Viewport { highlight_files: Some(files), .. } if !files.contains(&file_index));
        let highlighted = if options.highlight && should_highlight {
            highlight_cache.prefetch_highlighted_diff_shared(file, &options.theme, live)
        } else {
            None
        };
        let uncached_gap_source;
        let gap_source = match purpose {
            ReviewRowPurpose::Viewport {
                gap_geometries: Some(gaps),
                ..
            } => &gaps[file_index].gaps,
            _ => {
                uncached_gap_source = workdeck_review::review_gap_geometry_for_file(file);
                &uncached_gap_source
            }
        };
        let selected_source = options.source_presentation.text(file);
        let line_highlight_paint = line_highlights.get(&file.runtime_id).and_then(|marks| {
            build_line_highlight_paint_index(file, marks, options.tab_width, selected_source)
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
                    source,
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
                    source,
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
            if let Some(address) = gap_source.leading_gap(hunk_index) {
                if options.source_presentation.available(file) {
                    gap_rows.push((rows.len(), file_index, hunk_index));
                }
                rows.extend(source_gap_rows(
                    file,
                    address,
                    hunk_index,
                    layout,
                    options,
                    width,
                    selection.file_index == file_index
                        && selection.hunk_index == Some(address.hunk_index),
                    expanded_gaps,
                    highlighted_source.as_ref(),
                    line_highlight_paint.as_ref(),
                    file_index,
                    rows.len(),
                    &mut note_targets,
                    &mut line_cursors,
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
            let mut rendered = match layout {
                LayoutMode::Split => split_hunk_rows(
                    file,
                    file_index,
                    hunk,
                    hunk_index,
                    options,
                    width,
                    highlighted
                        .as_ref()
                        .and_then(|code| code.highlighted.get(hunk_index)),
                    comments,
                    file_selection,
                    selected_hunk,
                    line_highlight_paint.as_ref(),
                    purpose,
                    rows.len(),
                ),
                LayoutMode::Stack | LayoutMode::Auto => stack_hunk_rows(
                    file,
                    file_index,
                    hunk,
                    hunk_index,
                    options,
                    highlighted
                        .as_ref()
                        .and_then(|code| code.highlighted.get(hunk_index)),
                    comments,
                    width,
                    file_selection,
                    selected_hunk,
                    line_highlight_paint.as_ref(),
                    purpose,
                ),
            };
            if options.agent_notes {
                insert_agent_annotation_rows(
                    file,
                    hunk_index,
                    &mut rendered,
                    layout,
                    options,
                    width,
                );
            }
            let row_start = rows.len();
            line_cursors.extend(rendered.cursor_targets.iter().map(|(offset, target)| {
                ReviewLineCursor {
                    row: row_start + offset,
                    target: *target,
                }
            }));
            note_targets.extend(
                rendered
                    .targets
                    .into_iter()
                    .enumerate()
                    .filter_map(|(offset, target)| {
                        target.map(|target| (row_start + offset, target))
                    }),
            );
            note_bounds.extend(
                rendered
                    .note_bounds
                    .into_iter()
                    .map(|(note_id, top, height)| {
                        (note_id, (row_start.saturating_add(top), height))
                    }),
            );
            rows.extend(rendered.lines);
            hunk_heights.insert(
                (file_index, hunk_index),
                rows.len().saturating_sub(
                    *hunk_tops
                        .get(&(file_index, hunk_index))
                        .expect("hunk top was recorded before its rows"),
                ),
            );
        }
        if let Some(address) = gap_source.trailing_gap() {
            if options.source_presentation.available(file) {
                gap_rows.push((rows.len(), file_index, file.hunks.len()));
            }
            rows.extend(source_gap_rows(
                file,
                address,
                file.hunks.len(),
                layout,
                options,
                width,
                selection.file_index == file_index
                    && selection.hunk_index == Some(address.hunk_index),
                expanded_gaps,
                highlighted_source.as_ref(),
                line_highlight_paint.as_ref(),
                file_index,
                rows.len(),
                &mut note_targets,
                &mut line_cursors,
            ));
        }
    }
    ReviewRows {
        lines: rows,
        note_targets: note_targets.into_iter().collect(),
        note_bounds,
        line_cursors,
        file_tops,
        file_header_tops,
        file_body_tops,
        visible_file_indices,
        file_header_rows,
        gap_rows,
        hunk_tops,
        hunk_heights,
        file_view_component_hits,
    }
}

#[allow(clippy::too_many_arguments)]
fn append_extension_file_view_rows(
    rows: &mut Vec<Line<'static>>,
    hunk_tops: &mut std::collections::HashMap<(usize, usize), usize>,
    hunk_heights: &mut std::collections::HashMap<(usize, usize), usize>,
    note_bounds: &mut std::collections::HashMap<String, (usize, usize)>,
    file: &DiffFile,
    file_index: usize,
    selection: ReviewSelection,
    comments: &[ReviewComment],
    resolved: &ResolvedFileViewLayout,
    options: &ReviewOptions,
    width: usize,
    component_expanded: &BTreeSet<FileViewComponentStateKey>,
    component_hits: &mut Vec<FileViewComponentLogicalHit>,
    line_cursors: &mut Vec<ReviewLineCursor>,
    file_presentation_rendering: Option<&Mutex<FilePresentationRenderingController>>,
    extension_notifications: Option<&ExtensionNotificationHub>,
) -> bool {
    let mut notes = comments
        .iter()
        .filter(|comment| comment.anchor.file_key == file.key)
        .filter(|comment| comment.source != "user-draft")
        .filter(|comment| options.agent_notes || comment.source == "user")
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
                    extra: Default::default(),
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
    if options.agent_notes
        && let Some(context) = &file.agent
    {
        notes.extend(
            context
                .annotations
                .iter()
                .enumerate()
                .map(|(index, annotation)| {
                    let mut annotation = annotation.clone();
                    if !options.experimental {
                        annotation.markup = None;
                    }
                    VisibleFileViewNote {
                        id: annotation.id.as_ref().map_or_else(
                            || format!("annotation:{}:at:{index}", file.runtime_id),
                            |id| format!("annotation:{}:id:{id}", file.runtime_id),
                        ),
                        has_actions: annotation.editable,
                        annotation,
                        thread_depth: 0,
                    }
                }),
        );
    }
    let plan = build_file_view_render_plan(&resolved.validated.layout, &notes);
    if !plan.unresolved_note_ids.is_empty() {
        return false;
    }
    let geometry = measure_file_view_geometry(&resolved.validated, &plan.rows, width);
    let body_top = rows.len();
    for (plan_index, planned) in plan.rows.iter().enumerate() {
        let PlannedFileViewRow::FileViewRow {
            stable_alias_keys, ..
        } = planned
        else {
            continue;
        };
        let row = body_top.saturating_add(geometry.row_bounds[plan_index].top);
        line_cursors.extend(stable_alias_keys.iter().filter_map(|stable_key| {
            let target = line_stable_key_target(stable_key)?;
            Some(ReviewLineCursor {
                row,
                target: ReviewNoteTarget {
                    file_index,
                    hunk_index: target.hunk_index,
                    side: target.side,
                    line: target.line,
                },
            })
        }));
    }
    for (hunk_index, top) in &geometry.hunk_anchor_rows {
        hunk_tops.insert((file_index, *hunk_index), body_top.saturating_add(*top));
    }
    for (hunk_index, bounds) in &geometry.hunk_bounds {
        hunk_heights.insert((file_index, *hunk_index), bounds.height);
    }
    for note in &notes {
        if let Some(bounds) =
            geometry.bounds_for_stable_key(&workdeck_review::inline_note_stable_key(&note.id))
        {
            note_bounds.insert(
                note.id.clone(),
                (body_top.saturating_add(bounds.top), bounds.height),
            );
        }
    }
    let expanded_row_ids = component_expanded
        .iter()
        .filter(|state| state.file_id == file.runtime_id)
        .map(|state| state.row_id.clone())
        .collect::<BTreeSet<_>>();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX);
    let painted = paint_file_view(FileViewViewOptions {
        file,
        resolved: &resolved.validated,
        geometry: &geometry,
        cursor_highlight: None,
        selected_hunk_index: (selection.file_index == file_index)
            .then_some(selection.hunk_index)
            .flatten(),
        theme: &options.theme,
        visible_body_bounds: None,
        width,
        identity: FileViewPaintIdentity {
            extension_id: &resolved.extension_id,
            view_id: &resolved.view_id,
            registration_identity: resolved.registration_identity,
            layout_generation: resolved.layout_generation,
        },
        expanded_row_ids: &expanded_row_ids,
        now_ms,
    });
    report_file_view_row_failures(
        file_presentation_rendering,
        extension_notifications,
        &painted.failures,
    );
    for row in &painted.rows {
        if row.toggle_expanded_on_left_mouse_up
            && let Some(row_id) = &row.row_id
        {
            component_hits.push(FileViewComponentLogicalHit {
                state_key: FileViewComponentStateKey {
                    file_id: file.runtime_id.clone(),
                    row_id: row_id.clone(),
                },
                top: body_top.saturating_add(row.top),
                height: row.height,
            });
        }
    }
    rows.extend(painted.lines());
    true
}

fn report_file_view_row_failures(
    controller: Option<&Mutex<FilePresentationRenderingController>>,
    extension_notifications: Option<&ExtensionNotificationHub>,
    failures: &[workdeck_extension_api::FileViewRowFailure],
) {
    let Some(controller) = controller else {
        return;
    };
    let mut controller = controller
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for failure in failures {
        if let Some(warning) = controller.report_row_failure(failure)
            && let Some(notifications) = extension_notifications
        {
            notifications.notify(warning, ExtensionNotifyType::Warning);
        }
    }
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

#[allow(clippy::too_many_arguments)]
fn source_gap_rows(
    file: &DiffFile,
    address: ReviewGapAddress,
    gap_slot: usize,
    layout: LayoutMode,
    options: &ReviewOptions,
    width: u16,
    selected: bool,
    expanded_gaps: &BTreeSet<(String, usize)>,
    highlighted_source: Option<&workdeck_diff::HighlightedSourceCode>,
    line_highlights: Option<&LineHighlightPaintIndex>,
    file_index: usize,
    row_base: usize,
    note_targets: &mut Vec<(usize, ReviewNoteTarget)>,
    line_cursors: &mut Vec<ReviewLineCursor>,
) -> Vec<Line<'static>> {
    let side = review_expansion_side(file.change_kind);
    let expanded = expanded_gaps.contains(&(file.key.clone(), gap_slot));
    let status = options.source_presentation.expanded_status(file);
    let plan = plan_expanded_gap(&file.key, address, expanded, status, side);
    let mut rows = Vec::with_capacity(plan.lines.len().saturating_add(1));
    rows.push(source_gap_label(
        file,
        address,
        &plan.label,
        width,
        options.source_presentation.available(file),
        selected,
        &options.theme,
    ));
    for expanded_line in plan.lines {
        let start = rows.len();
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
        let target = ReviewNoteTarget {
            file_index,
            hunk_index: address.hunk_index,
            side,
            line: match side {
                ReviewSide::Old => expanded_line.old_line,
                ReviewSide::New => expanded_line.new_line,
            },
        };
        for row in start..rows.len() {
            let row = row_base.saturating_add(row);
            note_targets.push((row, target));
            line_cursors.push(ReviewLineCursor { row, target });
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

fn source_gap_label(
    file: &DiffFile,
    address: ReviewGapAddress,
    label: &str,
    width: u16,
    expandable: bool,
    selected: bool,
    theme: &AppTheme,
) -> Line<'static> {
    let key = workdeck_review::review_gap_id(address.position, address.hunk_index);
    let planned = PlannedReviewRow::DiffRow {
        key: key.clone(),
        stable_key: key.clone(),
        stable_alias_keys: Vec::new(),
        file_id: file.key.clone(),
        hunk_index: address.hunk_index,
        row: workdeck_diff::DiffRow::Collapsed {
            key,
            file_id: file.key.clone(),
            hunk_index: address.hunk_index,
            text: label.to_owned(),
            position: address.position,
            old_range: [
                address.old_range.start as usize,
                address.old_range.end as usize,
            ],
            new_range: [
                address.new_range.start as usize,
                address.new_range.end as usize,
            ],
        },
        anchor_id: None,
        note_guide_side: None,
    };
    paint_diff_meta_row(
        &planned,
        DiffMetaRowViewOptions {
            width: usize::from(width),
            theme,
            selected,
            show_hunk_headers: true,
            show_add_note_badge: false,
            enable_gap_toggle: expandable,
        },
    )
    .expect("collapsed gap rows always have metadata paint")
    .line
    .ratatui_line()
}

fn insert_agent_annotation_rows(
    file: &DiffFile,
    hunk_index: usize,
    rows: &mut TargetedHunkRows,
    layout: LayoutMode,
    options: &ReviewOptions,
    width: u16,
) {
    let Some(context) = &file.agent else { return };
    let mut insertions = Vec::new();
    for annotation in &context.annotations {
        let preferred =
            annotation_anchor(annotation).map(|anchor| workdeck_review::ReviewPreferredLine {
                side: anchor.side,
                line: anchor.line_number,
            });
        let anchor = workdeck_review::resolve_review_note_anchor(
            &file.hunks,
            workdeck_review::ReviewNoteAnchorInput {
                old_range: annotation.old_range,
                new_range: annotation.new_range,
                preferred,
                fallback_owner_hunk_index: preferred.and_then(|target| {
                    workdeck_review::review_gap_owner_hunk_index(
                        &file.hunks,
                        target.side,
                        target.line,
                    )
                }),
            },
        );
        if anchor.owner_hunk_index != Some(hunk_index) {
            continue;
        }
        let selected = preferred
            .and_then(|preferred| {
                rows.cursor_targets.iter().find(|(_, target)| {
                    target.side == preferred.side && target.line == preferred.line
                })
            })
            .or_else(|| rows.cursor_targets.first());
        let Some((selected_row, _)) = selected else {
            continue;
        };
        // Include wrapped continuations in the target's code block.
        let at = rows
            .cursor_targets
            .iter()
            .filter_map(|(row, _)| (*row > *selected_row).then_some(*row))
            .min()
            .unwrap_or(rows.lines.len());
        let mut annotation = annotation.clone();
        if !options.experimental {
            annotation.markup = None;
        }
        let mut view = AgentInlineNoteViewOptions::new(
            &annotation,
            layout,
            &options.theme,
            usize::from(width),
        );
        view.file = Some(file);
        view.anchor_side = preferred.map(|target| target.side);
        let painted = paint_agent_inline_note(&AgentInlineNoteViewState::default(), view);
        insertions.push((at, painted.ratatui_lines()));
    }
    // Reverse insertion preserves annotation order at shared anchors and leaves
    // yet-to-be-inserted source offsets valid.
    insertions.sort_by_key(|(at, _)| *at);
    for (at, lines) in insertions.into_iter().rev() {
        let height = lines.len();
        for (row, _) in &mut rows.cursor_targets {
            if *row >= at {
                *row += height;
            }
        }
        for (_, top, _) in &mut rows.note_bounds {
            if *top >= at {
                *top += height;
            }
        }
        rows.targets
            .splice(at..at, std::iter::repeat_n(None, height));
        rows.lines.splice(at..at, lines);
    }
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
    for annotation in context.annotations.iter().filter(|_| file.hunks.is_empty()) {
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
    purpose: ReviewRowPurpose,
) -> TargetedHunkRows {
    let mut rows = Vec::new();
    let mut targets = Vec::new();
    let mut cursor_targets = Vec::new();
    let mut note_bounds = Vec::new();
    let mut emphasis = vec![Vec::new(); hunk.lines.len()];
    let geometry_nowrap = purpose == ReviewRowPurpose::Geometry && !options.wrap_lines;
    for pair in plan_split_line_pairs(&hunk.lines)
        .into_iter()
        .filter(|_| !geometry_nowrap)
    {
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
        let line_rows = if geometry_nowrap {
            // Unwrapped code occupies exactly one row, regardless of clipping,
            // styles, Unicode width or horizontal offset. These rows never paint.
            vec![Line::default()]
        } else {
            stack_line_rows(
                line,
                options,
                highlighted
                    .and_then(|lines| lines.get(index))
                    .and_then(|line_highlight| line_highlight.for_stack(line.kind)),
                &emphasis[index],
                hunk_selected || line_is_selected(line, selection),
                width,
                line_highlight_ranges(line_highlights, line),
            )
        };
        let target = diff_line_note_target(file_index, hunk_index, line);
        cursor_targets.push((rows.len(), target));
        targets.extend(std::iter::repeat_n(Some(target), line_rows.len()));
        rows.extend(line_rows);
        let rendered_notes = comment_rows(
            file,
            line,
            comments,
            &options.theme,
            width,
            LayoutMode::Stack,
        );
        let note_start = rows.len();
        note_bounds.extend(
            rendered_notes
                .note_bounds
                .into_iter()
                .map(|(note_id, top, height)| (note_id, note_start.saturating_add(top), height)),
        );
        targets.extend(std::iter::repeat_n(
            Some(target),
            rendered_notes.lines.len(),
        ));
        rows.extend(rendered_notes.lines);
    }
    TargetedHunkRows {
        lines: rows,
        targets,
        cursor_targets,
        note_bounds,
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
    purpose: ReviewRowPurpose,
    row_offset: usize,
) -> TargetedHunkRows {
    let mut rows = Vec::new();
    let mut targets = Vec::new();
    let mut cursor_targets = Vec::new();
    let mut note_bounds = Vec::new();
    let pane_widths = resolve_diff_split_pane_widths(usize::from(width));
    let left_width = pane_widths.left_width;
    let right_width = pane_widths.right_width;
    let geometry_nowrap = purpose == ReviewRowPurpose::Geometry && !options.wrap_lines;
    let uncached_pairs;
    let pairs = match purpose {
        ReviewRowPurpose::Viewport {
            gap_geometries: Some(files),
            ..
        } => &files[file_index].split_pairs[hunk_index],
        _ => {
            uncached_pairs = plan_split_line_pairs(&hunk.lines);
            &uncached_pairs
        }
    };
    // Every pair contributes at least its ordinary unwrapped row in the common
    // path; wrapping and notes can grow beyond this initial capacity. Each source
    // line contributes one cursor (context lines are shared by both sides).
    rows.reserve(pairs.len());
    targets.reserve(pairs.len());
    cursor_targets.reserve(hunk.lines.len());
    for pair in pairs {
        let geometry_nowrap = geometry_nowrap
            || (!options.wrap_lines
                && matches!(purpose, ReviewRowPurpose::Viewport { start, end, .. }
                    if !(start..end).contains(&row_offset.saturating_add(rows.len()))));
        let old = pair.old_index.and_then(|index| hunk.lines.get(index));
        let new = pair.new_index.and_then(|index| hunk.lines.get(index));
        let emphasis = old
            .zip(new)
            .filter(|(old, new)| !std::ptr::eq(*old, *new))
            .filter(|_| !geometry_nowrap)
            .map(|(old, new)| {
                word_diff_ranges(
                    &expanded_line_content(old, options.tab_width),
                    &expanded_line_content(new, options.tab_width),
                )
            });
        let pair_rows = if geometry_nowrap {
            // Append the single geometry row directly below, without allocating
            // a temporary one-element vector for every offscreen pair.
            Vec::new()
        } else {
            split_pair_rows(
                SplitCellInput {
                    line: old,
                    highlighted: pair
                        .old_index
                        .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                        .and_then(|line| line.deletion.as_ref()),
                    emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.old),
                    line_highlights: old
                        .and_then(|line| line_highlight_ranges(line_highlights, line)),
                },
                SplitCellInput {
                    line: new,
                    highlighted: pair
                        .new_index
                        .and_then(|index| highlighted.and_then(|lines| lines.get(index)))
                        .and_then(|line| line.addition.as_ref()),
                    emphasis: emphasis.as_ref().map_or(&[], |ranges| &ranges.new),
                    line_highlights: new
                        .and_then(|line| line_highlight_ranges(line_highlights, line)),
                },
                options,
                left_width,
                right_width,
                hunk_selected
                    || old.is_some_and(|line| line_is_selected(line, selection))
                    || new.is_some_and(|line| line_is_selected(line, selection)),
            )
        };
        let cursor_row = rows.len();
        if pair.old_index == pair.new_index {
            if let Some(line) = new.or(old) {
                cursor_targets.push((
                    cursor_row,
                    diff_line_note_target(file_index, hunk_index, line),
                ));
            }
        } else {
            if let Some(line) = old {
                cursor_targets.push((
                    cursor_row,
                    diff_line_note_target(file_index, hunk_index, line),
                ));
            }
            if let Some(line) = new {
                cursor_targets.push((
                    cursor_row,
                    diff_line_note_target(file_index, hunk_index, line),
                ));
            }
        }
        let pair_target = new
            .or(old)
            .map(|line| diff_line_note_target(file_index, hunk_index, line));
        if geometry_nowrap {
            targets.push(pair_target);
            rows.push(Line::default());
        } else {
            targets.extend(std::iter::repeat_n(pair_target, pair_rows.len()));
            rows.extend(pair_rows);
        }
        if let Some(line) = old {
            let rendered_notes = comment_rows(
                file,
                line,
                comments,
                &options.theme,
                width,
                LayoutMode::Split,
            );
            let note_start = rows.len();
            note_bounds.extend(
                rendered_notes
                    .note_bounds
                    .into_iter()
                    .map(|(note_id, top, height)| {
                        (note_id, note_start.saturating_add(top), height)
                    }),
            );
            targets.extend(std::iter::repeat_n(
                Some(diff_line_note_target(file_index, hunk_index, line)),
                rendered_notes.lines.len(),
            ));
            rows.extend(rendered_notes.lines);
        }
        if pair.new_index != pair.old_index
            && let Some(line) = new
        {
            let rendered_notes = comment_rows(
                file,
                line,
                comments,
                &options.theme,
                width,
                LayoutMode::Split,
            );
            let note_start = rows.len();
            note_bounds.extend(
                rendered_notes
                    .note_bounds
                    .into_iter()
                    .map(|(note_id, top, height)| {
                        (note_id, note_start.saturating_add(top), height)
                    }),
            );
            targets.extend(std::iter::repeat_n(
                Some(diff_line_note_target(file_index, hunk_index, line)),
                rendered_notes.lines.len(),
            ));
            rows.extend(rendered_notes.lines);
        }
    }
    TargetedHunkRows {
        lines: rows,
        targets,
        cursor_targets,
        note_bounds,
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

fn comments_with_thread_draft(
    comments: &[ReviewComment],
    composer: Option<&ReviewNoteComposer>,
) -> Vec<ReviewComment> {
    let mut projected = comments.to_vec();
    if let Some(composer) = composer
        && let ReviewNoteComposerKind::Reply { parent_id } = &composer.kind
        && let Some(parent) = comments.iter().find(|comment| &comment.id == parent_id)
    {
        // Only sibling-guide projection sees this pending child; its editor is
        // inserted separately and it never enters the persistent review store.
        let mut draft = parent.clone();
        draft.id = composer.id.clone();
        draft.parent_id = Some(parent_id.clone());
        draft.source = "user-draft".into();
        projected.push(draft);
    }
    projected
}

fn saved_comment_thread(
    comment: &ReviewComment,
    comments: &[ReviewComment],
) -> VisibleAgentNoteThread {
    let mut state = workdeck_review::SemanticReviewState::new(
        Arc::new(workdeck_core::SemanticReviewDocument { files: Vec::new() }),
        true,
    );
    state.live_notes = comments
        .iter()
        .filter_map(|comment| {
            let mut normalized = comment.clone();
            normalized.hunk_index = comment.hunk_index.or(comment.anchor.owner_hunk_index);
            normalized.side = comment.side.or(comment.anchor.preferred_side);
            normalized.line = comment.line.or(comment.anchor.preferred_line);
            let mut stored = workdeck_review::live_comment_to_stored_note(
                &normalized,
                &comment.anchor.file_key,
                &[],
            )
            .ok()?;
            stored.note.parent_id = comment.parent_id.clone();
            stored.resolution = comment.resolution;
            Some(stored)
        })
        .collect();
    let selected = workdeck_review::select_visible_threaded_stored_review_notes(&state)
        .into_iter()
        .find(|note| note.threaded.entry.note.id == comment.id);
    VisibleAgentNoteThread {
        note_id: comment.id.clone(),
        parent_id: selected
            .as_ref()
            .and_then(|note| note.visible_parent_id.clone()),
        depth: selected.as_ref().map_or(0, |note| note.visible_depth),
        has_next_sibling: Some(
            selected
                .as_ref()
                .is_some_and(|note| note.has_next_visible_sibling),
        ),
        ancestor_has_next_sibling: selected
            .map_or_else(Vec::new, |note| note.visible_ancestor_has_next_sibling),
    }
}

/// The source hook depends on stored collections, not only renderable projections.
fn saved_extension_notes_identity(comments: &[ReviewComment]) -> String {
    let stored = comments
        .iter()
        .filter(|comment| comment.source != "user-draft")
        .collect::<Vec<_>>();
    workdeck_core::review_serialized_digest(&stored).expect("stored notes are serializable")
}

fn saved_extension_annotations(
    changeset: &Changeset,
    comments: &[ReviewComment],
    show_agent_notes: bool,
) -> BTreeMap<String, Vec<AgentAnnotation>> {
    let mut state = workdeck_review::SemanticReviewState::new(
        Arc::new(workdeck_core::SemanticReviewDocument { files: Vec::new() }),
        show_agent_notes,
    );
    for comment in comments
        .iter()
        .filter(|comment| comment.source != "user-draft")
    {
        let stored = workdeck_review::review_comment_to_stored_note(comment);
        if stored.note.source == workdeck_core::ReviewNoteSource::User {
            state.user_notes.push(stored);
        } else {
            state.live_notes.push(stored);
        }
    }
    let threaded = workdeck_review::select_threaded_stored_review_notes(&state);
    let visible = workdeck_review::select_visible_threaded_stored_review_notes(&state)
        .into_iter()
        .map(|entry| (entry.threaded.entry.note.id.clone(), entry))
        .collect::<BTreeMap<_, _>>();
    let files = changeset
        .files
        .iter()
        .map(|file| (file.key.clone(), file))
        .collect();
    workdeck_review::group_threaded_stored_notes_by_file_id(
        &threaded,
        &files,
        |note, path, depth, has_replies, _| {
            let visible = visible.get(&note.id);
            let projection = workdeck_review::ReviewNoteProjection {
                thread_depth: visible.map_or(depth, |entry| entry.visible_depth),
                has_replies,
                thread_guide: Some(workdeck_review::ReviewThreadGuide {
                    has_next_sibling: visible.is_some_and(|entry| entry.has_next_visible_sibling),
                    ancestor_has_next_sibling: visible.map_or_else(Vec::new, |entry| {
                        entry.visible_ancestor_has_next_sibling.clone()
                    }),
                }),
            };
            if note.source == workdeck_core::ReviewNoteSource::User {
                workdeck_review::stored_note_to_user_note(note, path, projection)
            } else {
                workdeck_review::stored_note_to_live_comment(note, path, projection)
            }
            .annotation()
        },
    )
}

fn saved_comment_annotation(comment: &ReviewComment) -> AgentAnnotation {
    use workdeck_core::LineRange;
    let preferred = comment
        .anchor
        .preferred_side
        .zip(comment.anchor.preferred_line);
    AgentAnnotation {
        extra: Default::default(),
        id: Some(comment.id.clone()),
        old_range: comment.anchor.old_range.or_else(|| {
            preferred
                .filter(|(side, _)| *side == ReviewSide::Old)
                .map(|(_, line)| LineRange {
                    start: line,
                    end: line,
                })
        }),
        new_range: comment.anchor.new_range.or_else(|| {
            preferred
                .filter(|(side, _)| *side == ReviewSide::New)
                .map(|(_, line)| LineRange {
                    start: line,
                    end: line,
                })
        }),
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
    }
}

fn paint_saved_comment(
    comment: &ReviewComment,
    comments: &[ReviewComment],
    file: &DiffFile,
    layout: LayoutMode,
    theme: &AppTheme,
    width: u16,
    hovered: bool,
) -> PaintedAgentInlineNote {
    let annotation = saved_comment_annotation(comment);
    let mut view = AgentInlineNoteViewOptions::new(&annotation, layout, theme, usize::from(width));
    view.file = Some(file);
    let thread = saved_comment_thread(comment, comments);
    view.thread = Some(&thread);
    view.anchor_side = comment.side.or(comment.anchor.preferred_side);
    view.actions = Some(VisibleAgentNoteActions {
        reply: true,
        edit: comment.editable,
        delete: comment.editable,
    });
    let mut state = AgentInlineNoteViewState::default();
    if hovered {
        state.enter_card();
    }
    paint_agent_inline_note(&state, view)
}

fn comment_rows(
    file: &DiffFile,
    line: &DiffLine,
    comments: &[ReviewComment],
    theme: &AppTheme,
    width: u16,
    layout: LayoutMode,
) -> RenderedCommentRows {
    let mut rows = Vec::new();
    let mut note_bounds = Vec::new();
    for comment in comments
        .iter()
        .filter(|comment| comment.source != "user-draft")
        .filter(|comment| comment.anchor.file_key == file.key)
        .filter(|comment| comment_matches_line(comment, line))
    {
        let note_top = rows.len();
        if comment.source == "user" {
            rows.extend(
                paint_saved_comment(comment, comments, file, layout, theme, width, false)
                    .ratatui_lines(),
            );
            note_bounds.push((comment.id.clone(), note_top, rows.len() - note_top));
            continue;
        }
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
        note_bounds.push((
            comment.id.clone(),
            note_top,
            rows.len().saturating_sub(note_top),
        ));
    }
    RenderedCommentRows {
        lines: rows,
        note_bounds,
    }
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
    let reserved = if options.wrap_lines {
        usize::from(CODE_ROW_ADD_NOTE_BADGE_WIDTH)
    } else {
        0
    };
    let mut new_lines = split_cell_lines(
        new,
        false,
        options,
        right_width.saturating_sub(reserved),
        selected,
    );
    for line in &mut new_lines {
        pad_spans(
            line,
            right_width,
            Style::default().bg(ratatui_theme_color(&options.theme.panel)),
        );
    }
    let height = old_lines.len().max(new_lines.len()).max(1);
    let empty_cell = |width, old| {
        split_cell_lines(
            SplitCellInput {
                line: None,
                highlighted: None,
                emphasis: &[],
                line_highlights: None,
            },
            old,
            options,
            width,
            selected,
        )
        .remove(0)
    };
    old_lines.resize_with(height, || empty_cell(left_width, true));
    new_lines.resize_with(height, || empty_cell(right_width, false));
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
        let mut run_start = 0;
        let mut run_style = None;
        for (index, character) in span.content.char_indices() {
            let emphasized = ranges
                .iter()
                .any(|range| range.start <= offset && offset < range.end);
            let style = if emphasized {
                span.style.bg(background).add_modifier(Modifier::BOLD)
            } else {
                span.style
            };
            if let Some(previous) = run_style.filter(|previous| *previous != style) {
                append_emphasis_run(&mut result, &span.content[run_start..index], previous);
                run_start = index;
            }
            run_style = Some(style);
            offset += character.len_utf8();
        }
        if let Some(style) = run_style {
            append_emphasis_run(&mut result, &span.content[run_start..], style);
        }
    }
    result
}

fn append_emphasis_run(result: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if let Some(last) = result.last_mut().filter(|last| last.style == style) {
        last.content.to_mut().push_str(text);
    } else {
        result.push(Span::styled(text.to_owned(), style));
    }
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

fn render_help(
    area: Rect,
    buffer: &mut Buffer,
    commands: &[HelpCommand],
    theme: &AppTheme,
    vertical_offset: usize,
) -> HelpDialogRenderMap {
    render_help_dialog(area, buffer, commands, theme, vertical_offset)
}

fn render_note_composer(_area: Rect, _buffer: &mut Buffer, app: &ReviewApp) {
    app.note_composer_actions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    let Some(composer) = app.note_composer.as_ref() else {
        app.note_composer_bounds.set(None);
        return;
    };
    app.note_composer_bounds.set(None);
    let Some(area) = app.review_bounds.get() else {
        return;
    };
    let rows = app.current_review_rows();
    let Some(&(top, height)) = rows.note_bounds.get(&composer.id) else {
        return;
    };
    let viewport = usize::from(
        area.height
            .saturating_sub(2 + u16::from(!app.options.pager)),
    );
    let scroll = app.scroll.min(rows.lines.len().saturating_sub(viewport));
    if top < scroll || top >= scroll.saturating_add(viewport) {
        return;
    }
    let layout = app.with_state(|state| state.resolved_layout(area.width));
    let painted = app.with_state(|state| {
        paint_note_composer(
            composer,
            area.width,
            layout,
            &app.options.theme,
            state.changeset().files.get(composer.target.file_index),
        )
    });
    app.note_composer_bounds.set(Some(Rect::new(
        area.x.saturating_add(painted.box_left as u16),
        area.y
            .saturating_add(2)
            .saturating_add((top - scroll) as u16),
        painted.box_width as u16,
        height.min(viewport.saturating_sub(top - scroll)) as u16,
    )));
    let mut actions = app
        .note_composer_actions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for hit in painted.action_hits {
        let bounds = Rect::new(
            area.x.saturating_add(hit.column_start as u16),
            area.y
                .saturating_add(2)
                .saturating_add((top - scroll + hit.row) as u16),
            hit.width as u16,
            1,
        );
        if bounds.y < area.bottom() {
            actions.push((bounds, hit.action));
        }
    }
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
        AgentAnnotation, AgentFileContext, ChangesetSource, CliInput, CommonOptions,
        FileSourceSnapshots, LineRange, SourceOrigin, SourceSnapshot, VcsDiffCommandInput,
    };
    use workdeck_diff::{create_two_files_patch, parse_patch};
    use workdeck_review::{CommentAnchor, ReviewComment};

    #[test]
    fn emphasis_runs_match_character_reference_for_unicode_and_overlapping_ranges() {
        fn reference(spans: Vec<Span<'static>>, ranges: &[Range<usize>]) -> Vec<Span<'static>> {
            if ranges.is_empty() {
                return spans;
            }
            let mut result: Vec<Span<'static>> = Vec::new();
            let mut offset = 0;
            for span in spans {
                for character in span.content.chars() {
                    let style = if ranges.iter().any(|range| range.contains(&offset)) {
                        span.style.bg(Color::Blue).add_modifier(Modifier::BOLD)
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
        for text in ["", "ab cd", "日🚀e\u{301}\tZ"] {
            for split in (0..=text.len()).filter(|index| text.is_char_boundary(*index)) {
                for alternate_style in [Style::default(), Style::default().fg(Color::Red)] {
                    let spans = vec![
                        Span::raw(&text[..split]),
                        Span::styled("", alternate_style),
                        Span::styled(&text[split..], alternate_style),
                    ];
                    assert_eq!(emphasize_spans(spans.clone(), &[], Color::Blue), spans);
                    // Include byte boundaries inside multibyte scalars, empty ranges,
                    // overlapping ranges, and ranges extending beyond the content.
                    for start in 0..=text.len() + 1 {
                        for end in start..=text.len() + 1 {
                            for ranges in [
                                std::iter::once(start..end).collect::<Vec<_>>(),
                                vec![end..end + 2, start..end],
                            ] {
                                assert_eq!(
                                    emphasize_spans(spans.clone(), &ranges, Color::Blue),
                                    reference(spans.clone(), &ranges),
                                    "text={text:?} split={split} ranges={ranges:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn changeset() -> Changeset {
        parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "test",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    #[test]
    fn borrowed_filter_fields_match_semantic_projection_without_render_metadata() {
        let mut file = changeset().files.remove(0);
        file.path = "src/Current.ts\r\n".into();
        file.previous_path = Some("old/Öld.ts\r\n".into());
        file.agent = Some(AgentFileContext {
            path: file.path.clone(),
            summary: Some("Rewrites note policy".into()),
            annotations: vec![],
        });
        let semantic = workdeck_core::project_review_file(&file, "filter-test", 17);
        for (query, expected) in [
            ("", true),
            (" \t", true),
            ("CURRENT", true),
            ("öLD", true),
            ("note policy", true),
            ("Current.ts old/Öld", true),
            ("Öld.ts Rewrites", true),
            ("unrelated", false),
            ("current.ts rewrites", false),
        ] {
            assert_eq!(
                diff_file_matches_filter(&file, query),
                expected,
                "{query:?}"
            );
            assert_eq!(
                diff_file_matches_filter(&file, query),
                workdeck_review::review_file_matches_filter(&semantic, query)
            );
        }
        file.agent = None;
        file.previous_path = None;
        assert!(!diff_file_matches_filter(&file, "note policy"));
        assert!(!diff_file_matches_filter(&file, "öld"));
        assert!(diff_file_matches_filter(&file, "current"));
    }

    #[test]
    fn deferred_file_view_input_preserves_order_when_control_returns_to_the_host() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.deferred_file_view_keys
            .push_back((1, KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.deferred_file_view_keys.len(), 2);
        assert!(!app.show_help);
        app.poll_extension_commands();
        assert!(app.show_help);
        assert_eq!(app.deferred_file_view_keys.len(), 1);
        app.poll_extension_commands();
        assert!(!app.show_help);
        assert!(app.deferred_file_view_keys.is_empty());
        assert!(!app.replaying_file_view_key);
    }

    fn background_for_rendered_text(buffer: &Buffer, needle: &str) -> Color {
        for y in buffer.area.y..buffer.area.bottom() {
            let mut text = String::new();
            let mut byte_columns = Vec::new();
            for x in buffer.area.x..buffer.area.right() {
                let cell = buffer.cell((x, y)).unwrap();
                if !cell.symbol().is_empty() {
                    byte_columns.push((text.len(), x));
                    text.push_str(cell.symbol());
                }
            }
            if let Some(byte) = text.find(needle)
                && let Some((_, x)) = byte_columns.iter().find(|(start, _)| *start == byte)
            {
                return buffer.cell((*x, y)).unwrap().bg;
            }
        }
        panic!("no rendered row contained {needle:?}");
    }

    fn background_for_symbol_on_text_row(buffer: &Buffer, row_needle: &str, symbol: &str) -> Color {
        for y in buffer.area.y..buffer.area.bottom() {
            let row = (buffer.area.x..buffer.area.right())
                .map(|x| buffer.cell((x, y)).unwrap().symbol())
                .collect::<String>();
            if row.contains(row_needle)
                && let Some(cell) = (buffer.area.x..buffer.area.right())
                    .map(|x| buffer.cell((x, y)).unwrap())
                    .find(|cell| cell.symbol() == symbol)
            {
                return cell.bg;
            }
        }
        panic!("no rendered {symbol:?} cell appeared on the {row_needle:?} row");
    }

    fn cursor_line_changeset() -> Changeset {
        parse_patch(
            "diff --git a/sample.ts b/sample.ts\n--- a/sample.ts\n+++ b/sample.ts\n@@ -1,5 +1,5 @@\n const alpha = 1;\n-const beta = 2;\n+const beta = 22222;\n const gamma = 3;\n const delta = 4;\n const epsilon = 5;\n",
            "test",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    fn sidebar_visibility_changeset() -> Changeset {
        let mut review = changeset();
        review.files[0].path = "src/ui/alpha.ts".into();
        review.files[0].key = "src/ui/alpha.ts".into();
        review.files[0].runtime_id = "alpha".into();
        review
    }

    fn sidebar_resize_changeset() -> Changeset {
        let mut review = sidebar_visibility_changeset();
        let mut beta = review.files[0].clone();
        beta.path = "src/ui/beta.ts".into();
        beta.key = "src/ui/beta.ts".into();
        beta.runtime_id = "beta".into();
        review.files.push(beta);
        review.refresh_review_identities();
        review
    }

    fn responsive_changeset() -> Changeset {
        let mut review = parse_patch(
            "diff --git a/alpha.ts b/alpha.ts\n--- a/alpha.ts\n+++ b/alpha.ts\n@@ -1 +1,2 @@\n-export const alpha = 1;\n+export const alpha = 2;\n+export const add = true;\ndiff --git a/beta.ts b/beta.ts\n--- a/beta.ts\n+++ b/beta.ts\n@@ -1 +1 @@\n-export const beta = 1;\n+export const betaValue = 1;\n",
            "changeset:responsive",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        review.summary = Some("Patch summary".into());
        review.agent_summary = Some("Changeset summary".into());
        review
    }

    fn assert_fixture_annotation_range(review: &Changeset, line: u32) {
        let annotation = &review.files[0].agent.as_ref().unwrap().annotations[0];
        assert_eq!(
            annotation.new_range,
            Some(LineRange {
                start: line,
                end: line
            })
        );
        assert!(!annotation.extra.contains_key("newRange"));
    }

    #[test]
    fn annotation_toggle_shows_notes_for_both_files_in_current_viewport() {
        let mut review = responsive_changeset();
        for file in &mut review.files {
            file.agent = Some(AgentFileContext {
                path: file.path.clone(),
                summary: Some(format!("{} note", file.path)),
                annotations: vec![
                    serde_json::from_value(serde_json::json!({
                        "new_range": if file.path == "alpha.ts" { LineRange { start: 2, end: 2 } } else { LineRange { start: 1, end: 1 } },
                        "summary": format!("Annotation for {}", file.path),
                        "rationale": format!("Why {} changed", file.path)
                    }))
                    .unwrap(),
                ],
            });
        }
        review.refresh_review_identities();
        assert_fixture_annotation_range(&review, 2);
        assert_eq!(
            review.files[1].agent.as_ref().unwrap().annotations[0].new_range,
            Some(LineRange { start: 1, end: 1 })
        );
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                agent_notes: false,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(240, 32)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(!initial.contains("Annotation for alpha.ts"));
        assert!(!initial.contains("Annotation for beta.ts"));
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        let frame = rendered_review_frame(&mut terminal, &app);
        for expected in [
            "Annotation for alpha.ts",
            "Why alpha.ts changed",
            "Annotation for beta.ts",
            "Why beta.ts changed",
        ] {
            assert!(frame.contains(expected), "missing {expected}:\n{frame}");
        }
    }

    fn watched_changeset(observed: bool, rationale: Option<&str>) -> Changeset {
        let added = if observed {
            "+export const answer = 42;\n+export const observed = true;\n"
        } else {
            "+export const answer = 42;\n"
        };
        let mut review = parse_patch(
            &format!(
                "diff --git a/after.ts b/after.ts\n--- a/after.ts\n+++ b/after.ts\n@@ -1 +1,{} @@\n-export const answer = 41;\n{added}",
                if observed { 2 } else { 1 }
            ),
            "changeset:watch",
            "before.ts ↔ after.ts",
            ChangesetSource::Files {
                left: "before.ts".into(),
                right: "after.ts".into(),
            },
        )
        .unwrap();
        if let Some(summary) = rationale {
            review.files[0].agent = Some(AgentFileContext {
                path: "after.ts".into(),
                summary: None,
                annotations: vec![AgentAnnotation {
                    extra: Default::default(),
                    id: None,
                    old_range: None,
                    new_range: Some(LineRange { start: 2, end: 2 }),
                    summary: summary.into(),
                    rationale: None,
                    markup: None,
                    tags: Vec::new(),
                    confidence: None,
                    source: None,
                    title: None,
                    author: None,
                    created_at: None,
                    updated_at: None,
                    editable: false,
                }],
            });
            review.refresh_review_identities();
        }
        review
    }

    fn reload_content_changeset(marker: &str) -> Changeset {
        parse_patch(
            &format!(
                "diff --git a/after.ts b/after.ts\n--- a/after.ts\n+++ b/after.ts\n@@ -1 +1,2 @@\n-export const answer = 41;\n+export const answer = 42;\n+export const {marker} = true;\n"
            ),
            "changeset:reload-content",
            "before.ts ↔ after.ts",
            ChangesetSource::Files {
                left: "before.ts".into(),
                right: "after.ts".into(),
            },
        )
        .unwrap()
    }

    fn reload_attention_changeset(include_alpha: bool) -> Changeset {
        let alpha = if include_alpha {
            "diff --git a/alpha.ts b/alpha.ts\n--- a/alpha.ts\n+++ b/alpha.ts\n@@ -1 +1 @@\n-export const alpha = 1;\n+export const alpha = 100;\n"
        } else {
            ""
        };
        parse_patch(
            &format!(
                "{alpha}diff --git a/bravo.ts b/bravo.ts\n--- a/bravo.ts\n+++ b/bravo.ts\n@@ -1 +1 @@\n-export const bravo = 1;\n+export const bravo = 2;\n"
            ),
            "changeset:reload-attention",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    fn install_cached_test_file_view(app: &ReviewApp, selected_file_id: &str) -> String {
        let files = app.with_state(|state| state.changeset().files.clone());
        let registration_identity = 77;
        let registered = Arc::new(RegisteredFileView {
            extension_id: "probe".into(),
            view_id: "preview".into(),
            interactive_mode: false,
        });
        let key = registered_file_view_key(&registered);
        let mut runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.file_views = vec![LiveFileViewRegistration {
            extension_index: 0,
            registration_identity,
            title: "Preview".into(),
            view: registered,
        }];
        for file in files {
            runtime.file_view_match_cache.insert(
                FileViewMatchCacheKey {
                    file_id: file.runtime_id,
                    content_identity: file.content_identity,
                    registration_identity,
                },
                true,
            );
        }
        runtime.file_view_selections =
            select_file_view(&runtime.file_view_selections, selected_file_id, Some(&key));
        key
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
    fn saved_extension_identity_tracks_unprojected_notes_but_not_drafts() {
        let document = changeset();
        let file = &document.files[0];
        let mut orphan = saved_comment(&file.key, "orphan", "original");
        orphan.resolution = workdeck_review::ReviewNoteResolution::Orphaned;
        let mut absent = saved_comment("absent", "absent", "original");
        let original = vec![orphan.clone(), absent.clone()];
        assert!(saved_extension_annotations(&document, &original, true).is_empty());
        let identity = saved_extension_notes_identity(&original);
        assert_eq!(identity, saved_extension_notes_identity(&original.clone()));
        orphan.summary = "edited orphan".into();
        let edited = vec![orphan.clone(), absent.clone()];
        assert!(saved_extension_annotations(&document, &edited, true).is_empty());
        assert_ne!(identity, saved_extension_notes_identity(&edited));
        absent.summary = "edited absent note".into();
        assert_ne!(
            saved_extension_notes_identity(&edited),
            saved_extension_notes_identity(&[orphan, absent])
        );
        let mut with_draft = original.clone();
        let mut draft = saved_comment(&file.key, "draft", "draft");
        draft.source = "user-draft".into();
        with_draft.push(draft);
        assert_eq!(identity, saved_extension_notes_identity(&with_draft));
        with_draft.last_mut().unwrap().summary = "edited draft".into();
        assert_eq!(identity, saved_extension_notes_identity(&with_draft));
    }

    #[test]
    fn saved_extension_projection_preserves_thread_metadata_defaults_and_exclusions() {
        let document = changeset();
        let file = &document.files[0];
        let mut root = saved_comment(&file.key, "root", "root");
        root.source = "user".into();
        root.editable = true;
        let mut reply = saved_comment(&file.key, "reply", "reply");
        reply.parent_id = Some(root.id.clone());
        let mut sibling = root.clone();
        sibling.id = "sibling".into();
        let mut orphan = root.clone();
        orphan.id = "orphan".into();
        orphan.resolution = workdeck_review::ReviewNoteResolution::Orphaned;
        let mut draft = root.clone();
        draft.id = "draft".into();
        draft.source = "user-draft".into();
        let missing = saved_comment("absent", "missing", "missing");
        let annotations = saved_extension_annotations(
            &document,
            &[root, reply, sibling, orphan, draft, missing],
            true,
        );
        let notes = &annotations[&file.runtime_id];
        assert_eq!(notes.len(), 3);
        let root = notes
            .iter()
            .find(|note| note.id.as_deref() == Some("root"))
            .unwrap();
        assert_eq!(root.author.as_deref(), Some("user"));
        assert_eq!(root.created_at.as_deref(), Some("1970-01-01T00:00:00.000Z"));
        assert!(root.editable);
        assert_eq!(root.extra["reviewNoteId"], "root");
        assert_eq!(root.extra["filePath"], file.path);
        assert_eq!(root.extra["hunkIndex"], 0);
        assert_eq!(root.extra["side"], "new");
        assert_eq!(root.extra["line"], 1);
        assert_eq!(root.extra["hasReplies"], true);
        // Hunk draws sibling connectors only for notes with a visible parent.
        assert_eq!(root.extra["hasNextSibling"], false);
        let reply = notes
            .iter()
            .find(|note| note.id.as_deref() == Some("reply"))
            .unwrap();
        assert_eq!(reply.source.as_deref(), Some("mcp"));
        assert_eq!(reply.extra["parentId"], "root");
        assert_eq!(reply.extra["threadDepth"], 1);
        assert_eq!(
            reply.extra["ancestorHasNextSibling"],
            serde_json::json!([false])
        );
        assert_eq!(reply.extra["semanticallyStored"], true);
    }

    #[test]
    fn saved_extension_projection_collapses_hidden_ancestors_without_dropping_metadata() {
        let document = changeset();
        let file = &document.files[0];
        let parent = saved_comment(&file.key, "agent-parent", "parent");
        let mut reply = saved_comment(&file.key, "user-reply", "reply");
        reply.source = "user".into();
        reply.parent_id = Some(parent.id.clone());
        let annotations = saved_extension_annotations(&document, &[parent, reply], false);
        let notes = &annotations[&file.runtime_id];
        assert_eq!(
            notes.len(),
            2,
            "visibility does not remove extension metadata"
        );
        let reply = notes
            .iter()
            .find(|note| note.id.as_deref() == Some("user-reply"))
            .unwrap();
        assert_eq!(reply.extra["parentId"], "agent-parent");
        assert_eq!(reply.extra["threadDepth"], 0);
        assert_eq!(reply.extra["hasNextSibling"], false);
        assert_eq!(reply.extra["ancestorHasNextSibling"], serde_json::json!([]));
    }

    #[test]
    fn saved_annotation_preserves_preferred_source_line_without_hunk_ranges() {
        let mut comment = saved_comment("file", "note", "source note");
        comment.anchor.old_range = None;
        comment.anchor.new_range = None;
        comment.anchor.preferred_side = Some(ReviewSide::New);
        comment.anchor.preferred_line = Some(7);
        let annotation = saved_comment_annotation(&comment);
        assert_eq!(annotation.new_range, Some(LineRange { start: 7, end: 7 }));
        assert_eq!(annotation.old_range, None);
        assert_eq!(annotation.id.as_deref(), Some("note"));
        assert_eq!(annotation.summary, "source note");
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

    pub(super) fn navigation_changeset(files: Vec<(String, String, String)>) -> Changeset {
        let patch = files
            .iter()
            .map(|(path, before, after)| create_two_files_patch(path, before, after, 3))
            .collect::<String>();
        parse_patch(
            &patch,
            "pty-navigation",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    pub(super) fn numbered_exports(
        start: usize,
        count: usize,
        offset: usize,
        padded: bool,
    ) -> String {
        (start..start.saturating_add(count))
            .map(|line| {
                let label = if padded {
                    format!("{line:02}")
                } else {
                    line.to_string()
                };
                format!(
                    "export const line{label} = {};",
                    line.saturating_add(offset)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    fn multi_hunk_navigation_changeset() -> Changeset {
        let before = numbered_exports(1, 80, 0, false);
        let mut after = before.lines().map(str::to_owned).collect::<Vec<_>>();
        after[0] = "export const line1 = 100;".into();
        for line in 60..=65 {
            after[line - 1] = format!("export const line{line} = {line}00;");
        }
        navigation_changeset(vec![("after.ts".into(), before, after.join("\n") + "\n")])
    }

    fn agent_navigation_changeset() -> Changeset {
        let alpha_before = numbered_exports(1, 80, 0, false);
        let mut alpha_after = alpha_before.lines().map(str::to_owned).collect::<Vec<_>>();
        alpha_after[0] = "export const line1 = 1001;".into();
        alpha_after[59] = "export const line60 = 6000;".into();

        let beta_before = numbered_exports(81, 20, 0, false);
        let mut beta_after = beta_before.lines().map(str::to_owned).collect::<Vec<_>>();
        beta_after[0] = "export const line81 = 8100;".into();

        let gamma_before = numbered_exports(101, 80, 0, false);
        let mut gamma_after = gamma_before.lines().map(str::to_owned).collect::<Vec<_>>();
        gamma_after[0] = "export const line101 = 10100;".into();
        gamma_after[59] = "export const line160 = 16000;".into();

        navigation_changeset(vec![
            (
                "alpha.ts".into(),
                alpha_before,
                alpha_after.join("\n") + "\n",
            ),
            ("beta.ts".into(), beta_before, beta_after.join("\n") + "\n"),
            (
                "gamma.ts".into(),
                gamma_before,
                gamma_after.join("\n") + "\n",
            ),
        ])
    }

    fn navigation_comment(
        file_key: &str,
        id: &str,
        hunk_index: usize,
        line: u32,
        summary: &str,
    ) -> ReviewComment {
        let mut comment = saved_comment(file_key, id, summary);
        comment.hunk_index = Some(hunk_index);
        comment.line = Some(line);
        comment.anchor.new_range = Some(LineRange {
            start: line,
            end: line,
        });
        comment.anchor.preferred_line = Some(line);
        comment.anchor.intersecting_hunk_indices = vec![hunk_index];
        comment.anchor.owner_hunk_index = Some(hunk_index);
        comment
    }

    fn cross_file_hunk_navigation_changeset() -> Changeset {
        let long_before = (1..=342)
            .map(|line| format!("line {line:03}"))
            .collect::<Vec<_>>();
        let mut long_after = long_before.clone();
        for line in [
            2, 21, 41, 61, 81, 101, 121, 141, 161, 181, 201, 221, 241, 261, 281, 301, 321, 341,
        ] {
            long_after[line - 1] = format!("line {line:03} changed");
        }
        let short_before = [
            "// hunk 0 - at the very top of the file".to_owned(),
            "export const top = 1;".to_owned(),
            String::new(),
            String::new(),
        ]
        .into_iter()
        .chain((1..=25).map(|line| format!("// filler {line}")))
        .chain([
            "// hunk 1 - mid-file".to_owned(),
            "export const mid = 3;".to_owned(),
        ])
        .collect::<Vec<_>>();
        let mut short_after = short_before.clone();
        short_after[1] = "export const top = 2;".into();
        short_after[30] = "export const mid = 4;".into();

        navigation_changeset(vec![
            (
                "long-file.txt".into(),
                long_before.join("\n") + "\n",
                long_after.join("\n") + "\n",
            ),
            (
                "short-file.ts".into(),
                short_before.join("\n") + "\n",
                short_after.join("\n") + "\n",
            ),
        ])
    }

    fn sidebar_jump_navigation_changeset() -> Changeset {
        navigation_changeset(
            ["alpha", "beta", "gamma", "delta", "epsilon"]
                .into_iter()
                .map(|name| {
                    (
                        format!("{name}.ts"),
                        format!("export const {name} = 1;\n"),
                        format!("export const {name}Value = 2;\nexport const {name}Only = true;\n"),
                    )
                })
                .collect(),
        )
    }

    fn pinned_header_navigation_changeset() -> Changeset {
        navigation_changeset(vec![
            (
                "first.ts".into(),
                numbered_exports(1, 16, 0, true),
                numbered_exports(1, 16, 100, true),
            ),
            (
                "second.ts".into(),
                numbered_exports(17, 16, 0, true),
                numbered_exports(17, 16, 100, true),
            ),
        ])
    }

    fn overflowing_changeset(file_count: usize) -> Changeset {
        let mut changes = changeset();
        changes.files[0].runtime_id = "file-0".into();
        for index in 1..file_count {
            let mut file = changes.files[0].clone();
            file.key = format!("file-key-{index}");
            file.runtime_id = format!("file-{index}");
            file.path = format!("src/file-{index}.rs");
            changes.files.push(file);
        }
        changes
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
    fn provisional_extension_runtime_cannot_publish_event_context_before_commit() {
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                command_cwd: Some(PathBuf::from("/repo/committed")),
                ..ReviewOptions::default()
            },
        );
        let slot = app.extension_event_context_provider.clone();
        assert_eq!(
            slot.context(Vec::new()).unwrap().cwd,
            PathBuf::from("/repo/committed")
        );

        let prospective = changeset();
        let provisional = ProvisionalExtensionPaneRuntime::new(ExtensionPaneRuntime::new(
            Vec::new(),
            &prospective.files,
        ));
        assert_eq!(
            slot.context(Vec::new()).unwrap().cwd,
            PathBuf::from("/repo/committed")
        );
        drop(provisional);
        assert_eq!(
            slot.context(Vec::new()).unwrap().cwd,
            PathBuf::from("/repo/committed")
        );
    }

    #[test]
    fn frozen_app_extension_runtime_and_command_control_oracles_map_every_case() {
        let runtime: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/app-extension-runtime.json"
        ))
        .unwrap();
        assert_eq!(
            runtime["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(runtime["source"]["baseline"]["bytes"], 3_800);
        assert_eq!(runtime["source"]["baseline"]["lines"], 111);
        assert_eq!(runtime["source"]["stable"]["presence"], "absent");
        assert_eq!(runtime["oracleRuns"]["baseline"]["passed"], 2);
        assert_eq!(runtime["oracleRuns"]["baseline"]["failed"], 0);
        assert_eq!(runtime["testMappings"].as_array().unwrap().len(), 2);

        let controls: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/app-extension-command-controls.json"
        ))
        .unwrap();
        assert_eq!(
            controls["source"]["baseline"]["blob"],
            "cb89d00d4a0b2b7d750afa8bb57f5dae6b3c238a"
        );
        assert_eq!(
            controls["source"]["stable"]["blob"],
            controls["source"]["baseline"]["blob"]
        );
        for pin in ["baseline", "stable"] {
            assert_eq!(controls["oracleRuns"][pin]["passed"], 1);
            assert_eq!(controls["oracleRuns"][pin]["failed"], 0);
            assert_eq!(controls["oracleRuns"][pin]["expectCalls"], 9);
        }
        assert_eq!(controls["testMappings"].as_array().unwrap().len(), 1);

        for oracle in [&runtime, &controls] {
            assert!(
                oracle["testMappings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|mapping| {
                        mapping["upstream"]
                            .as_str()
                            .is_some_and(|name| !name.is_empty())
                            && mapping["rustTests"].as_array().is_some_and(|tests| {
                                !tests.is_empty()
                                    && tests.iter().all(|test| {
                                        test.as_str()
                                            .is_some_and(|selector| selector.contains(".rs#"))
                                    })
                            })
                    })
            );
        }
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
    fn one_review_producer_owns_initial_and_reloaded_publications() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let producer = app.review_producer();
        let initial = producer.get_publication_address();
        assert_eq!(producer.get_publication().document.files.len(), 1);

        let mut replacement = changeset();
        replacement.title = "Reloaded working tree".into();
        replacement.files[0].path = "renamed.rs".into();
        replacement.files[0].refresh_identity();
        app.reload_with_reason(replacement, SessionReloadReason::Watch, false);

        let reloaded = producer.get_publication_address();
        assert_ne!(reloaded.generation, initial.generation);
        assert_eq!(
            producer.get_publication().document.files[0].path,
            "renamed.rs"
        );
        assert_eq!(app.review_producer().get_publication_address(), reloaded);
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
    fn extension_confirm_uses_shared_footer_hover_and_separator_hit_semantics() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::OpenConfirmDialog {
                id: "confirm".into(),
                title: "Ship this change?".into(),
                body: "Review the generated artifact.".into(),
                confirm_label: "ship".into(),
                cancel_label: Some("cancel".into()),
            }],
        );

        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let map = app
            .extension_confirm_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .expect("confirm dialog hit map");
        assert_eq!(map.action_hits.len(), 2);
        assert_eq!(map.action_hits[0].key_label, "enter/y");
        assert_eq!(map.action_hits[1].key_label, "esc/n");

        let first = map.action_hits[0].bounds;
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Moved,
            column: first.x,
            row: first.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.extension_confirm_hovered_action_key.as_deref(),
            Some("enter/y")
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[first.as_position()].bg,
            ratatui_theme_color(&app.options.theme.accent_muted)
        );

        let separator_column = map.action_hits[0].bounds.right().saturating_add(1);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: separator_column,
            row: first.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current(),
            Some(ExtensionDialogRequest::Confirm(_))
        ));
    }

    #[test]
    fn extension_select_mouse_accepts_the_exact_visible_row_and_backdrop_cancels() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::OpenSelectDialog {
                id: "target".into(),
                title: "Deploy where?".into(),
                options: (0..12).map(|index| format!("Option {index}")).collect(),
            }],
        );
        {
            let mut runtime = app
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.dialogs.pick_option(8);
        }
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let plan = app
            .extension_select_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .expect("select dialog hit map");
        assert_eq!(plan.window_start, 3);
        let target = plan
            .item_hits
            .iter()
            .find(|hit| hit.index == 9)
            .unwrap()
            .bounds;
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: target.x,
            row: target.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current()
                .is_none()
        );

        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::OpenSelectDialog {
                id: "cancel".into(),
                title: "Cancel me".into(),
                options: vec!["one".into()],
            }],
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current()
                .is_none()
        );
    }

    #[test]
    fn extension_input_mouse_actions_submit_live_text_or_cancel_from_close() {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::OpenInputDialog {
                id: "branch".into(),
                title: "Branch name?".into(),
                placeholder: "feature/...".into(),
                initial: None,
            }],
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let plan = app
            .extension_input_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .expect("input dialog hit map");
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("qx")
        );
        let submit = plan.action_hits[0].bounds;
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: submit.x,
            row: submit.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current()
                .is_none()
        );

        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::OpenInputDialog {
                id: "cancel".into(),
                title: "Cancel input?".into(),
                placeholder: String::new(),
                initial: None,
            }],
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let close = app
            .extension_input_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(|plan| plan.modal.close)
            .unwrap();
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: close.x,
            row: close.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .dialogs
                .current()
                .is_none()
        );
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
        let external_quit = Arc::new(AtomicBool::new(false));
        let app = ReviewApp::new(
            review,
            ReviewOptions {
                repo: Some(root.path().to_owned()),
                review_input: Some(writable_input()),
                external_quit_signal: Some(Arc::clone(&external_quit)),
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

        external_quit.store(true, Ordering::Release);
        let refused = app
            .write_extension_workspace_document_with(&dialog, |_, _| {
                panic!("shutdown must refuse a write before its irreversible boundary")
            })
            .unwrap_err();
        assert_eq!(
            refused,
            WorkspaceWriteFailure::Unavailable(
                "The review reloaded before this extension operation could finish.".into()
            )
        );
        external_quit.store(false, Ordering::Release);

        let shared_state = app.shared_state();
        app.write_extension_workspace_document_with(&dialog, |path, text| {
            // Once this writer is entered, the operation owns the synchronous
            // filesystem boundary. A simultaneous quit cannot falsify its
            // result or tear down the app until the owner thread returns.
            external_quit.store(true, Ordering::Release);
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

    pub(super) fn rendered_review_frame(
        terminal: &mut Terminal<TestBackend>,
        app: &ReviewApp,
    ) -> String {
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in buffer.area.y..buffer.area.bottom() {
            let mut x = buffer.area.x;
            while x < buffer.area.right() {
                let symbol = buffer.cell((x, y)).unwrap().symbol();
                rendered.push_str(symbol);
                // A terminal does not print the buffer cells covered by a wide
                // glyph. TestBackend can retain old symbols in these cells.
                x = x.saturating_add(measure_text_width(symbol).max(1) as u16);
            }
            rendered.push('\n');
        }
        rendered
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
                extension_trust_handler: Some(ExtensionTrustHandler::new(move |root, decision| {
                    observed.lock().unwrap().push((root.to_owned(), decision));
                    Ok(())
                })),
                ..ReviewOptions::default()
            },
        );
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.process_extension_trust_request(true));
        app.reload(two_file_changeset());

        assert_eq!(
            *calls.lock().unwrap(),
            [(
                PathBuf::from("/repo/alpha"),
                workdeck_extension_host::TrustDecision::Trusted,
            )]
        );
        assert_eq!(app.with_state(|state| state.changeset().files.len()), 2);
        assert_eq!(app.status.as_deref(), Some("review reloaded"));
        assert!(app.options.pending_extension_trust_repo_root.is_none());
    }

    #[test]
    fn repository_extension_denial_and_nonreloadable_trust_never_load_code() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&calls);
        let handler = ExtensionTrustHandler::new(move |root, decision| {
            observed.lock().unwrap().push((root.to_owned(), decision));
            Ok(())
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
        assert!(!denied.process_extension_trust_request(false));
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
        assert!(!deferred.process_extension_trust_request(false));
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
                cursor_line: CursorLineMode::Off,
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

        // Three right-hand cells remain reserved for the add-note affordance,
        // including when it is hidden, so wrapped rows do not jump on hover.
        assert_eq!(rows.lines.len(), 7);
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

        let mut unavailable_changeset = changeset.clone();
        unavailable_changeset.files[0].sources = FileSourceSnapshots::default();
        let unavailable = build_review_rows(
            &unavailable_changeset,
            &[],
            ReviewSelection::default(),
            LayoutMode::Stack,
            &options,
            80,
            &mut highlights,
            &BTreeSet::new(),
        );
        let unavailable_text = unavailable
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(unavailable_text.contains("··· 2 unchanged lines ···"));
        assert!(!unavailable_text.contains('▾'));
        assert!(unavailable.gap_rows.is_empty());

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

        for selected in [false, true] {
            for width in [0, 1, 2, 12, 80] {
                let painted = build_review_rows(
                    &changeset,
                    &[],
                    ReviewSelection {
                        hunk_index: selected.then_some(0),
                        ..ReviewSelection::default()
                    },
                    LayoutMode::Stack,
                    &options,
                    width,
                    &mut highlights,
                    &BTreeSet::new(),
                );
                let line = &painted.lines[painted.gap_rows[0].0];
                assert_eq!(line.width(), usize::from(width));
                if width > 0 {
                    assert_eq!(line.spans[0].content, diff_rail_marker());
                    let rail_color = if selected {
                        neutral_rail_color(&options.theme).to_owned()
                    } else {
                        dim_rail_color(neutral_rail_color(&options.theme), &options.theme)
                    };
                    assert_eq!(
                        line.spans[0].style.fg,
                        Some(ratatui_theme_color(&rail_color))
                    );
                    assert!(line.spans.iter().all(|span| {
                        span.style.bg == Some(ratatui_theme_color(&options.theme.panel_alt))
                    }));
                }
                if width == 80 {
                    assert_eq!(
                        line.spans[1].style.fg,
                        Some(ratatui_theme_color(&options.theme.muted))
                    );
                }
            }
        }

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
    fn live_source_presentation_renders_pending_loading_errors_and_loaded_rows() {
        use workdeck_review::{ReviewSourceErrorReason, ReviewSourceStatus};
        let mut changeset = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3 +3 @@\n-old\n+new\n",
            "lazy-source",
            "Lazy source",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        changeset.files[0].set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: Some("snapshot-one".into()),
        }));
        let original = changeset.clone();
        let file = &changeset.files[0];
        let mut options = ReviewOptions {
            highlight: false,
            line_numbers: false,
            ..ReviewOptions::default()
        };
        options.source_presentation.pending(file);
        let gaps = BTreeSet::from([(file.key.clone(), 0)]);
        let mut highlights = HighlightedDiffRuntime::default();
        for layout in [LayoutMode::Stack, LayoutMode::Split] {
            for (status, label, loaded) in [
                (None, "2 unchanged lines", false),
                (
                    Some(ReviewSourceStatus::Loading),
                    "Loading 2 unchanged lines",
                    false,
                ),
                (
                    Some(ReviewSourceStatus::Error { reason: None }),
                    "Could not load 2 unchanged lines",
                    false,
                ),
                (
                    Some(ReviewSourceStatus::Error {
                        reason: Some(ReviewSourceErrorReason::TooLarge),
                    }),
                    "Source too large to expand 2 unchanged lines",
                    false,
                ),
                (
                    Some(ReviewSourceStatus::Loaded {
                        text: "one\ntwo\nnew\n".into(),
                    }),
                    "Hide 2 unchanged lines",
                    true,
                ),
            ] {
                if let Some(status) = status {
                    options.source_presentation.set_status(file, status);
                } else {
                    options.source_presentation.pending(file);
                }
                let rows = build_review_rows(
                    &changeset,
                    &[],
                    ReviewSelection::default(),
                    layout,
                    &options,
                    120,
                    &mut highlights,
                    &gaps,
                );
                let text = rows
                    .lines
                    .iter()
                    .map(Line::to_string)
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(text.contains(label), "{text}");
                assert_eq!(text.contains("one"), loaded);
                assert!(!rows.gap_rows.is_empty());
                if loaded {
                    assert!(rows.note_targets.values().any(|target| target.line == 1));
                }
            }
        }
        assert_eq!(
            changeset, original,
            "loading presentation must not mutate provider snapshots"
        );
        let mut changed = changeset.clone();
        changed.files[0].set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: Some("replacement".into()),
        }));
        assert!(!options.source_presentation.available(&changed.files[0]));
        options.source_presentation.reconcile(&changed.files);
        assert!(options.source_presentation.text(file).is_none());
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
    fn moved_line_tints_reach_final_cells_in_every_layout_and_wrap_mode() {
        fn colored_text(buffer: &Buffer, background: Color) -> String {
            let mut output = String::new();
            for y in buffer.area.y..buffer.area.bottom() {
                for x in buffer.area.x..buffer.area.right() {
                    let cell = buffer.cell((x, y)).unwrap();
                    if cell.bg == background {
                        output.push_str(cell.symbol());
                    }
                }
                output.push('\n');
            }
            output
        }

        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/moved-lines.json"))
                .unwrap();
        assert_eq!(
            oracle["source"],
            serde_json::json!({
                "path": "test/pty/moved-lines.test.ts",
                "blob": "3a78c2b73e50701e8311979274a40c3da46b6639",
                "sha256": "b49979219e63ded4e109e9732e7b87bffed6d300838c8839450b6f0f5916131b",
                "bytes": 2_463,
                "lines": 65,
                "baseline_commit": "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                "stable_commit": "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
                "pins_identical": true
            })
        );
        for pin in ["baseline", "stable"] {
            assert_eq!(oracle["oracle_runs"][pin]["passed"], 4);
            assert_eq!(oracle["oracle_runs"][pin]["failed"], 0);
            assert_eq!(oracle["oracle_runs"][pin]["expect_calls"], 32);
        }
        assert_eq!(
            oracle["terminal"],
            serde_json::json!({"columns": 160, "rows": 40})
        );
        assert_eq!(oracle["matrix"].as_array().unwrap().len(), 4);
        assert_eq!(oracle["source_tests"].as_array().unwrap().len(), 4);

        let moved_block = [
            "MOVED BLOCK ALPHA",
            "MOVED BLOCK BRAVO",
            "MOVED BLOCK CHARLIE",
            "MOVED BLOCK DELTA",
        ];
        let plain_addition = "brand new destination line";
        let mut changeset = parse_patch(
            concat!(
                "diff --git a/source.txt b/source.txt\n",
                "--- a/source.txt\n",
                "+++ b/source.txt\n",
                "@@ -1,7 +1,3 @@\n",
                " source header one\n",
                " source header two\n",
                "-MOVED BLOCK ALPHA\n",
                "-MOVED BLOCK BRAVO\n",
                "-MOVED BLOCK CHARLIE\n",
                "-MOVED BLOCK DELTA\n",
                " source footer\n",
                "diff --git a/destination.txt b/destination.txt\n",
                "--- a/destination.txt\n",
                "+++ b/destination.txt\n",
                "@@ -1,2 +1,7 @@\n",
                " destination header one\n",
                " destination header two\n",
                "+MOVED BLOCK ALPHA\n",
                "+MOVED BLOCK BRAVO\n",
                "+MOVED BLOCK CHARLIE\n",
                "+MOVED BLOCK DELTA\n",
                "+brand new destination line\n",
            ),
            "moved-lines",
            "Moved lines",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        for line in changeset
            .files
            .iter_mut()
            .flat_map(|file| &mut file.hunks)
            .flat_map(|hunk| &mut hunk.lines)
        {
            line.moved = line.content.starts_with("MOVED BLOCK");
        }
        changeset.refresh_review_identities();

        for (layout, wrap_lines) in [
            (LayoutMode::Stack, true),
            (LayoutMode::Stack, false),
            (LayoutMode::Split, true),
            (LayoutMode::Split, false),
        ] {
            let app = ReviewApp::new(
                changeset.clone(),
                ReviewOptions {
                    layout,
                    wrap_lines,
                    highlight: false,
                    cursor_line: CursorLineMode::Off,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
            terminal
                .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
                .unwrap();
            let theme = &app.options.theme;
            let moved = colored_text(
                terminal.backend().buffer(),
                ratatui_theme_color(&theme.moved_added_bg),
            );
            let added = colored_text(
                terminal.backend().buffer(),
                ratatui_theme_color(&theme.added_bg),
            );
            let removed = colored_text(
                terminal.backend().buffer(),
                ratatui_theme_color(&theme.removed_bg),
            );
            for line in moved_block {
                assert!(
                    moved.contains(line),
                    "{layout:?} wrap={wrap_lines}: {moved}"
                );
            }
            assert!(!added.contains("MOVED BLOCK"));
            assert!(!removed.contains("MOVED BLOCK"));
            assert!(added.contains(plain_addition));
            assert!(!moved.contains(plain_addition));
        }
    }

    #[test]
    fn large_live_highlight_keeps_page_navigation_responsive_and_settles_colored() {
        fn visible_worker_line_indexes(frame: &str) -> Vec<usize> {
            frame
                .match_indices("workerLine")
                .filter_map(|(start, _)| {
                    let suffix = &frame[start + "workerLine".len()..];
                    let digits = suffix
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>();
                    (!digits.is_empty()).then(|| digits.parse().unwrap())
                })
                .collect()
        }

        let mut patch = concat!(
            "diff --git a/after.ts b/after.ts\n",
            "new file mode 100644\n",
            "--- /dev/null\n",
            "+++ b/after.ts\n",
            "@@ -0,0 +1,8000 @@\n",
        )
        .to_owned();
        for index in 0..8_000 {
            patch.push_str(&format!("+export const workerLine{index} = {index};\n"));
        }
        let changeset = parse_patch(
            &patch,
            "large-highlight",
            "Large highlight",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let mut app = ReviewApp::new(
            changeset,
            ReviewOptions {
                layout: LayoutMode::Stack,
                sidebar: false,
                cursor_line: CursorLineMode::Off,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        let last_initial = visible_worker_line_indexes(&initial)
            .into_iter()
            .max()
            .expect("initial plain-text worker row");

        let started = Instant::now();
        app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        let navigated = rendered_review_frame(&mut terminal, &app);
        let navigation_elapsed = started.elapsed();
        // Hunk's one-second threshold measures its production Bun build. Keep that exact gate for
        // optimized Rust while allowing instrumentation-heavy debug row construction to prove the
        // same nonblocking transition without turning compiler mode into the behavior under test.
        let navigation_budget = if cfg!(debug_assertions) {
            Duration::from_secs(3)
        } else {
            Duration::from_secs(1)
        };
        assert!(
            navigation_elapsed < navigation_budget,
            "PageDown repaint took {navigation_elapsed:?}"
        );
        assert!(
            visible_worker_line_indexes(&navigated)
                .into_iter()
                .any(|index| index > last_initial)
        );

        let keyword = Color::Rgb(0xff, 0x7b, 0x72);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            rendered_review_frame(&mut terminal, &app);
            let colored = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .filter(|cell| cell.fg == keyword)
                .map(|cell| cell.symbol())
                .collect::<String>();
            if colored.contains("export") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "native highlighting did not settle"
            );
            std::thread::yield_now();
        }
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
    fn mounted_cursor_line_marks_initial_stack_split_and_off_rows() {
        let render = |layout, cursor_line| {
            let backend = TestBackend::new(200, 16);
            let mut terminal = Terminal::new(backend).unwrap();
            let app = ReviewApp::new(
                cursor_line_changeset(),
                ReviewOptions {
                    layout,
                    cursor_line,
                    sidebar: false,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            );
            terminal
                .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
                .unwrap();
            terminal
        };

        let stack = render(LayoutMode::Stack, CursorLineMode::Row);
        let stack_buffer = stack.backend().buffer();
        assert_ne!(
            background_for_rendered_text(stack_buffer, "alpha = 1"),
            background_for_rendered_text(stack_buffer, "gamma = 3")
        );
        assert_eq!(
            background_for_rendered_text(stack_buffer, "gamma = 3"),
            background_for_rendered_text(stack_buffer, "delta = 4")
        );

        let split = render(LayoutMode::Split, CursorLineMode::Row);
        assert_ne!(
            background_for_rendered_text(split.backend().buffer(), "alpha = 1"),
            background_for_rendered_text(split.backend().buffer(), "gamma = 3")
        );

        let off = render(LayoutMode::Stack, CursorLineMode::Off);
        assert_eq!(
            background_for_rendered_text(off.backend().buffer(), "alpha = 1"),
            background_for_rendered_text(off.backend().buffer(), "gamma = 3")
        );
    }

    #[test]
    fn mounted_cursor_line_navigation_retains_removed_and_added_tints() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let backend = TestBackend::new(200, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            cursor_line_changeset(),
            ReviewOptions {
                layout: LayoutMode::Stack,
                cursor_line: CursorLineMode::Row,
                sidebar: false,
                highlight: false,
                theme: theme.clone(),
                ..ReviewOptions::default()
            },
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert_eq!(
            background_for_rendered_text(terminal.backend().buffer(), "beta = 2"),
            ratatui_theme_color(&cursor_line_highlight_background(
                stack_cell_palette(RowCellKind::Deletion, &theme, false).content_background,
                &theme,
            ))
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert_eq!(
            background_for_rendered_text(terminal.backend().buffer(), "beta = 22222"),
            ratatui_theme_color(&cursor_line_highlight_background(
                stack_cell_palette(RowCellKind::Addition, &theme, false).content_background,
                &theme,
            ))
        );
    }

    #[test]
    fn mounted_cursor_line_marks_a_wrapped_cjk_chunk() {
        let content = format!("export const message = \"{}\";", "日本語".repeat(80));
        let patch = format!(
            "diff --git a/wrapped.ts b/wrapped.ts\n--- /dev/null\n+++ b/wrapped.ts\n@@ -0,0 +1 @@\n+{content}\n"
        );
        let changeset = parse_patch(
            &patch,
            "test",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let render = |cursor_line| {
            let backend = TestBackend::new(120, 16);
            let mut terminal = Terminal::new(backend).unwrap();
            let app = ReviewApp::new(
                changeset.clone(),
                ReviewOptions {
                    layout: LayoutMode::Split,
                    cursor_line,
                    wrap_lines: true,
                    sidebar: false,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            );
            terminal
                .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
                .unwrap();
            terminal
        };
        let marked = render(CursorLineMode::Row);
        let plain = render(CursorLineMode::Off);
        assert_ne!(
            background_for_symbol_on_text_row(
                marked.backend().buffer(),
                "export const message",
                "日",
            ),
            background_for_symbol_on_text_row(
                plain.backend().buffer(),
                "export const message",
                "日",
            )
        );
    }

    #[test]
    fn frozen_app_host_cursor_line_oracle_maps_both_pins_and_each_source_test() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/app-host-cursor-line.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 7_643);
        assert_eq!(
            oracle["source"]["baseline"]["blob"],
            oracle["source"]["stable"]["blob"]
        );
        assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 7);
        assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 7);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 7);
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
                None,
                None,
                None,
                ReviewRowPurpose::Paint,
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
    fn user_note_composer_edits_and_replies_with_stable_public_identity() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let file_key = app.with_state(|state| state.changeset().files[0].key.clone());
        let mut original = saved_comment(&file_key, "user:stable-1", "original body");
        original.source = "user".into();
        original.editable = true;
        original.file_path = Some("a.rs".into());
        app.with_state(|state| state.add_comment(original).unwrap());
        app.observed_extension_events.clear();

        assert!(app.builtin_command_availability().can_edit_active_note);
        assert!(app.builtin_command_availability().can_reply_to_active_note);
        app.apply_builtin_command_action(AppCommandAction::EditActiveNote);
        let composer = app.note_composer.as_ref().unwrap();
        assert_eq!(
            composer.kind,
            ReviewNoteComposerKind::Edit {
                target_note_id: "user:stable-1".into(),
                parent_id: None,
            }
        );
        assert_eq!(composer.body, "original body");
        let composer = app.note_composer.as_mut().unwrap();
        composer.body = "edited body".into();
        composer.cursor = composer.body.chars().count();
        app.save_note_composer();
        let comments = app.with_state(|state| state.comments().to_vec());
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, "user:stable-1");
        assert_eq!(comments[0].summary, "edited body");
        assert!(
            app.observed_extension_events
                .iter()
                .any(|(_, event, payload)| {
                    event == "note_edited"
                        && payload["note"]["id"] == "user:stable-1"
                        && payload["note"]["body"] == "edited body"
                        && payload["note"]["draft"] == false
                })
        );

        app.observed_extension_events.clear();
        app.apply_builtin_command_action(AppCommandAction::ReplyToActiveNote);
        assert_eq!(
            app.note_composer.as_ref().unwrap().kind,
            ReviewNoteComposerKind::Reply {
                parent_id: "user:stable-1".into()
            }
        );
        let composer = app.note_composer.as_mut().unwrap();
        composer.body = "reply body".into();
        composer.cursor = composer.body.chars().count();
        app.save_note_composer();
        let comments = app.with_state(|state| state.comments().to_vec());
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[1].parent_id.as_deref(), Some("user:stable-1"));
        assert_eq!(comments[1].summary, "reply body");
        assert!(
            app.observed_extension_events
                .iter()
                .any(|(_, event, payload)| {
                    event == "note_created"
                        && payload["note"]["parentId"] == "user:stable-1"
                        && payload["note"]["body"] == "reply body"
                })
        );
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
    fn opted_in_pane_renders_both_sides_of_the_live_host_owned_current_line() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                sidebar: false,
                highlight: false,
                extension_panes: vec![ExtensionPaneView {
                    extension_id: "current-line".into(),
                    pane: PaneRegistration {
                        id: "inspect".into(),
                        title: "Current line".into(),
                        placement: PanePlacement::Right,
                        default_open: true,
                        preferred_size: Some(30),
                        width: None,
                        height: None,
                        replaces: None,
                        current_line: true,
                        available: false,
                    },
                    content: ViewNode::Column {
                        children: vec![
                            ViewNode::CurrentLine {
                                side: ExtensionFileSide::Old,
                                width: 24,
                            },
                            ViewNode::CurrentLine {
                                side: ExtensionFileSide::New,
                                width: 24,
                            },
                        ],
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
        assert!(rendered.contains("old"), "pane frame: {rendered:?}");
        assert!(rendered.contains("new"), "pane frame: {rendered:?}");
    }

    #[test]
    fn live_pane_failure_quarantines_one_identity_and_restores_a_replaced_files_slot() {
        let mut pane = bundled_files_pane().clone();
        pane.id = "replacement".into();
        pane.replaces = Some(WORKDECK_FILES_PANE_KEY.into());
        let registered = RegisteredExtensionPane::new("probe", pane.clone());
        let live = LivePaneRegistration {
            key: registered.key(),
            extension_index: 0,
            extension_id: "probe".into(),
            pane,
            registered: Arc::clone(&registered),
        };
        let mut runtime = ExtensionPaneRuntime::default();
        runtime.open.insert(live.key.clone());

        let failure = runtime
            .contain_pane_render_failure(&live, "renderer exploded")
            .unwrap();
        assert_eq!(
            failure.warning,
            "Extension probe pane \"replacement\" failed rendering • renderer exploded"
        );
        assert_eq!(failure.fallback, ExtensionPaneFallback::None);
        assert!(!runtime.open.contains(&live.key));
        assert!(runtime.force_builtin_files_sidebar);
        assert!(
            runtime
                .failed_pane_registration_ids
                .contains(&registered.identity)
        );
        assert!(
            runtime
                .contain_pane_render_failure(&live, "again")
                .is_none()
        );

        let fixed = RegisteredExtensionPane::new("probe", live.pane.clone());
        assert_eq!(registered.key(), fixed.key());
        assert_ne!(registered.identity, fixed.identity);
        assert!(
            !runtime
                .failed_pane_registration_ids
                .contains(&fixed.identity)
        );

        let backend = TestBackend::new(220, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .force_builtin_files_sidebar = true;
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert!(app.sidebar_bounds.get().is_some());
        app.toggle_files_pane_role();
        assert!(
            !app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .force_builtin_files_sidebar
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
    fn pane_input_flattening_selects_the_focused_row_editor_and_placeholder() {
        let view = ViewNode::Row {
            children: vec![
                ViewNode::Text {
                    text: "prompt: ".into(),
                    style: ViewStyle::default(),
                },
                ViewNode::Input {
                    id: "inactive".into(),
                    value: String::new(),
                    placeholder: Some("first".into()),
                    focused: false,
                },
                ViewNode::Input {
                    id: "active".into(),
                    value: "界".into(),
                    placeholder: None,
                    focused: true,
                },
            ],
            gap: 1,
        };
        let mut lines = Vec::new();
        let mut actions = Vec::new();
        let mut inputs = Vec::new();
        flatten_view(&view, 0, None, &mut lines, &mut actions, &mut inputs, None);

        assert_eq!(lines[0].to_string(), "prompt:  first 界");
        assert_eq!(inputs[0].as_ref().unwrap().input_id, "active");
        assert!(inputs[0].as_ref().unwrap().focused);
        assert_eq!(pane_input_display("", Some("placeholder")), "placeholder");
        assert_eq!(pane_input_display("value", Some("ignored")), "value");
    }

    #[test]
    fn pane_input_cursor_yields_to_every_higher_priority_overlay_owner() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .focused_pane_input = Some(FocusedExtensionPaneInput {
            pane_key: "probe:bottom".into(),
            registration_identity: 1,
            extension_index: 0,
            extension_id: "probe".into(),
            pane_id: "bottom".into(),
            input_id: "prompt".into(),
            value: "j界".into(),
            cursor: 2,
            bounds: Rect::new(10, 7, 20, 1),
            prefix_cells: 2,
        });
        assert_eq!(
            app.extension_pane_input_cursor_position(),
            Some(Position::new(15, 7))
        );

        app.show_help = true;
        assert_eq!(app.extension_pane_input_cursor_position(), None);
        app.show_help = false;
        app.themes.selector_open = true;
        assert_eq!(app.extension_pane_input_cursor_position(), None);
        app.themes.selector_open = false;
        app.focus = Focus::Filter;
        assert_eq!(app.extension_pane_input_cursor_position(), None);
        app.focus = Focus::Review;

        let menus = app.app_menus();
        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .menu
            .open(&menus, MenuId::File);
        assert_eq!(app.extension_pane_input_cursor_position(), None);
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
    fn pty_comment_navigation_resumes_from_an_unannotated_hunk_in_stream_order() {
        let review = agent_navigation_changeset();
        let alpha_key = review.files[0].key.clone();
        let gamma_key = review.files[2].key.clone();
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                agent_notes: true,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        app.with_state(|state| {
            state
                .add_comment(navigation_comment(
                    &alpha_key,
                    "alpha-navigation",
                    1,
                    60,
                    "Alpha note for navigation.",
                ))
                .unwrap();
            state
                .add_comment(navigation_comment(
                    &gamma_key,
                    "gamma-navigation",
                    1,
                    60,
                    "Gamma note for navigation.",
                ))
                .unwrap();
        });
        let mut terminal = Terminal::new(TestBackend::new(160, 14)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        for label in ["View", "Navigate", "Agent", "Help"] {
            assert!(initial.contains(label), "{initial}");
        }

        app.handle_key(KeyEvent::new(KeyCode::Char('}'), KeyModifiers::NONE));
        let alpha = rendered_review_frame(&mut terminal, &app);
        assert!(alpha.contains("Alpha note for navigation."), "{alpha}");
        assert!(!alpha.contains("Maximum update depth exceeded"));
        let alpha_rows = app.current_review_rows();
        let (alpha_note_top, _) = alpha_rows.note_bounds["alpha-navigation"];
        let viewport = usize::from(
            app.review_height
                .get()
                .saturating_sub(2 + u16::from(!app.options.pager))
                .max(1),
        );
        assert_eq!(
            alpha_note_top.saturating_sub(app.review_scroll()),
            2_usize.max(viewport / 4)
        );
        assert_eq!(
            app.with_state(|state| state.selection()),
            ReviewSelection {
                file_index: 0,
                hunk_index: Some(1),
                side: None,
                line: None,
            }
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        assert_eq!(app.with_state(|state| state.selection().file_index), 1);
        app.handle_key(KeyEvent::new(KeyCode::Char('}'), KeyModifiers::NONE));
        let gamma = rendered_review_frame(&mut terminal, &app);
        assert!(gamma.contains("Gamma note for navigation."), "{gamma}");
        assert!(!gamma.contains("Alpha note for navigation."), "{gamma}");
        assert!(!gamma.contains("Maximum update depth exceeded"));
        assert_eq!(
            app.with_state(|state| state.selection()),
            ReviewSelection {
                file_index: 2,
                hunk_index: Some(1),
                side: None,
                line: None,
            }
        );
    }

    #[test]
    fn pty_real_hunk_navigation_jumps_to_later_hunks_in_the_review_stream() {
        let mut app = ReviewApp::new(
            multi_hunk_navigation_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(104, 12)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(initial.contains("line1 = 100"), "{initial}");
        assert!(!initial.contains("line60 = 6000"), "{initial}");

        app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        let second = rendered_review_frame(&mut terminal, &app);
        assert!(second.contains("line60 = 6000"), "{second}");
        assert!(!second.contains("line1 = 100"), "{second}");
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
    }

    #[test]
    fn content_bottom_jump_remains_authoritative_after_hunk_navigation() {
        let mut app = ReviewApp::new(
            cross_file_hunk_navigation_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 16)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::SHIFT));
        for _ in 0..3 {
            let frame = rendered_review_frame(&mut terminal, &app);
            assert!(frame.contains("export const mid = 4;"), "{frame}");
            assert!(!frame.contains("line 021 changed"), "{frame}");
        }
    }

    #[test]
    fn pty_backward_cross_file_hunk_navigation_reveals_the_immediate_predecessor() {
        let mut app = ReviewApp::new(
            cross_file_hunk_navigation_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 16)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        let mut reached_short_file_top_hunk = false;
        let mut reached_short_file_mid_hunk = false;
        for _ in 0..24 {
            app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
            let frame = rendered_review_frame(&mut terminal, &app);
            let selection = app.with_state(|state| state.selection());
            if selection.file_index == 1 && selection.hunk_index == Some(0) {
                reached_short_file_top_hunk = true;
                assert!(frame.contains("export const top = 2;"), "{frame}");
                assert!(!frame.contains("export const mid = 4;"), "{frame}");
            }
            if frame.contains("export const mid = 4;") {
                reached_short_file_mid_hunk = true;
                break;
            }
        }
        assert!(reached_short_file_top_hunk);
        assert!(reached_short_file_mid_hunk);
        assert_eq!(
            app.with_state(|state| state.selection()),
            ReviewSelection {
                file_index: 1,
                hunk_index: Some(1),
                side: None,
                line: None,
            }
        );

        for _ in 0..2 {
            app.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
        }
        let backward = rendered_review_frame(&mut terminal, &app);
        assert!(backward.contains("line 341 changed"), "{backward}");
        assert!(!backward.contains("line 002 changed"), "{backward}");
        assert_eq!(app.with_state(|state| state.selection().file_index), 0);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(17)
        );
    }

    #[test]
    fn pty_hunk_navigation_round_trips_between_distant_hunks() {
        let mut app = ReviewApp::new(
            multi_hunk_navigation_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(104, 12)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(initial.contains("line1 = 100"), "{initial}");
        assert!(!initial.contains("line60 = 6000"), "{initial}");

        app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        let second = rendered_review_frame(&mut terminal, &app);
        assert!(second.contains("line60 = 6000"), "{second}");
        assert!(!second.contains("line1 = 100"), "{second}");

        app.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
        let first = rendered_review_frame(&mut terminal, &app);
        assert!(first.contains("line1 = 100"), "{first}");
        assert!(!first.contains("line60 = 6000"), "{first}");
    }

    #[test]
    fn pty_sidebar_selection_jumps_the_main_pane_without_collapsing_the_stream() {
        let mut app = ReviewApp::new(
            sidebar_jump_navigation_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(initial.contains("alphaOnly = true"), "{initial}");
        assert!(initial.contains("betaValue = 2"), "{initial}");
        assert!(!initial.contains("deltaOnly = true"), "{initial}");

        let delta = app
            .sidebar_file_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|hit| hit.file_index == 3)
            .copied()
            .expect("delta sidebar hit");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: delta.bounds.x,
            row: delta.bounds.y,
            modifiers: KeyModifiers::NONE,
        });
        let jumped = rendered_review_frame(&mut terminal, &app);
        assert!(jumped.contains("deltaValue = 2"), "{jumped}");
        assert!(jumped.contains("deltaOnly = true"), "{jumped}");
        assert!(!jumped.contains("alphaOnly = true"), "{jumped}");
        assert!(jumped.match_indices("epsilon.ts").count() >= 2, "{jumped}");
        assert_eq!(app.with_state(|state| state.selection().file_index), 3);
        assert_eq!(app.with_state(|state| state.changeset().files.len()), 5);
    }

    #[test]
    fn pty_sidebar_file_click_pins_that_file_header_to_the_review_top() {
        let mut app = ReviewApp::new(
            pinned_header_navigation_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 10)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(initial.contains("first.ts"));
        assert!(initial.contains("second.ts"));

        for _ in 0..16 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        let scrolled = rendered_review_frame(&mut terminal, &app);
        assert!(scrolled.contains("line08 = 108"), "{scrolled}");
        assert!(scrolled.contains("first.ts"), "{scrolled}");

        let second = app
            .sidebar_file_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|hit| hit.file_index == 1)
            .copied()
            .expect("second sidebar hit");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: second.bounds.x,
            row: second.bounds.y,
            modifiers: KeyModifiers::NONE,
        });
        let pinned = rendered_review_frame(&mut terminal, &app);
        assert!(pinned.contains("second.ts"), "{pinned}");
        assert!(pinned.contains("line17 = 117"), "{pinned}");
        assert_eq!(pinned.match_indices("first.ts").count(), 1, "{pinned}");
        assert_eq!(app.with_state(|state| state.selection().file_index), 1);
    }

    #[test]
    fn frozen_hunk_pty_navigation_oracle_maps_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/pty-navigation.json"
        ))
        .unwrap();
        assert_eq!(oracle["runtime"], "Bun 1.3.14");
        assert_eq!(
            oracle["source"]["baseline"],
            serde_json::json!({
                "commit": "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                "blob": "c66db15e3344541a1420f56d059772ce38f1a526",
                "sha256": "c195e80a6c96ab6d21d3cfb3033f74edf51bd7413534f8f8c6c6651ce93741e8",
                "bytes": 7_994,
                "lines": 257
            })
        );
        assert_eq!(
            oracle["source"]["stable"],
            serde_json::json!({
                "commit": "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
                "blob": "3a2dd989ad4d6b44d9137f19b49e3f38d8954f9e",
                "sha256": "7d4130b3fa8ec97a4ced7946a31e605ca9fd7088de33aed16502764780743b1f",
                "bytes": 7_972,
                "lines": 257
            })
        );
        for pin in ["baseline", "stable"] {
            assert_eq!(oracle["oracle_runs"][pin]["passed"], 6);
            assert_eq!(oracle["oracle_runs"][pin]["failed"], 0);
            assert_eq!(oracle["oracle_runs"][pin]["expect_calls"], 31);
        }
        assert_eq!(oracle["pin_delta"].as_array().unwrap().len(), 1);

        let implementation = include_str!("lib.rs");
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 6);
        let mut source_tests = BTreeSet::new();
        for mapping in mappings {
            assert!(source_tests.insert(mapping["source_test"].as_str().unwrap()));
            let evidence = mapping["evidence"].as_array().unwrap();
            assert!(!evidence.is_empty());
            for item in evidence {
                assert_eq!(item["file"], "crates/workdeck-tui/src/lib.rs");
                let test = item["test"].as_str().unwrap();
                let function = test
                    .strip_prefix("tests::")
                    .expect("TUI evidence names the test module");
                assert!(implementation.contains(&format!("fn {function}()")));
            }
        }
    }

    #[test]
    fn next_hunk_gives_destination_file_the_review_header_after_scrolling() {
        let mut review = navigation_changeset(vec![
            (
                "first.ts".into(),
                numbered_exports(1, 16, 0, true),
                numbered_exports(1, 16, 100, true),
            ),
            (
                "second.ts".into(),
                numbered_exports(17, 16, 0, true),
                numbered_exports(17, 16, 100, true),
            ),
        ]);
        for file in &mut review.files {
            file.agent = Some(
                serde_json::from_value(serde_json::json!({
                    "path":file.path, "summary":format!("{} note", file.path),
                    "annotations":[{"new_range":{"start":2,"end":2},
                        "summary":format!("Annotation for {}", file.path),
                        "rationale":format!("Why {} changed", file.path)}]
                }))
                .unwrap(),
            );
        }
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(220, 10)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        for _ in 0..10 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            rendered_review_frame(&mut terminal, &app);
        }
        assert!(rendered_review_frame(&mut terminal, &app).contains("first.ts"));
        app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("second.ts"), "{frame}");
        assert_eq!(frame.matches("first.ts").count(), 1, "{frame}");
        assert_eq!(app.with_state(|state| state.selection().file_index), 1);
    }

    #[test]
    fn sidebar_shortcut_opens_hidden_tree_in_pager_and_narrow_review() {
        for (pager, width) in [(true, 220), (false, 159)] {
            let mut app = ReviewApp::new(
                responsive_changeset(),
                ReviewOptions {
                    pager,
                    ..Default::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            for expected in [1, 2, 1] {
                let frame = rendered_review_frame(&mut terminal, &app);
                assert_eq!(
                    frame.matches("alpha.ts").count(),
                    expected,
                    "pager={pager}: {frame}"
                );
                if pager {
                    assert!(
                        !frame.contains("File  View  Navigate  Agent  Help"),
                        "{frame}"
                    );
                }
                app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
            }
        }
    }

    #[test]
    fn sidebar_toggle_removes_and_restores_the_second_file_path_occurrence() {
        let mut app = ReviewApp::new(responsive_changeset(), ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(240, 24)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert_eq!(initial.matches("alpha.ts").count(), 2, "{initial}");
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let hidden = rendered_review_frame(&mut terminal, &app);
        assert_eq!(hidden.matches("alpha.ts").count(), 1, "{hidden}");
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let restored = rendered_review_frame(&mut terminal, &app);
        assert_eq!(restored.matches("alpha.ts").count(), 2, "{restored}");
    }

    #[test]
    fn draft_blur_restores_sidebar_shortcut_without_discarding_draft() {
        let mut app = ReviewApp::new(responsive_changeset(), ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(240, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        let before = rendered_review_frame(&mut terminal, &app);
        assert!(before.contains("Draft note"));
        let count = before.matches("beta.ts").count();
        assert!(count > 1, "{before}");
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column: 6,
                row: 4,
                modifiers: KeyModifiers::NONE,
            });
        }
        assert!(rendered_review_frame(&mut terminal, &app).contains("Draft note"));
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let after = rendered_review_frame(&mut terminal, &app);
        assert!(after.contains("Draft note"));
        assert!(after.matches("beta.ts").count() < count, "{after}");
        assert!(app.note_composer.as_ref().unwrap().body.is_empty());
        assert!(!app.note_composer.as_ref().unwrap().focused);
        assert!(
            app.status_filter_cursor_position(Rect::new(0, 0, 240, 24))
                .is_none()
        );
        app.handle_paste("must not enter a blurred draft");
        assert!(app.note_composer.as_ref().unwrap().body.is_empty());
        let bounds = app.note_composer_bounds.get().unwrap();
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column: bounds.x + 2,
                row: bounds.y + 2,
                modifiers: KeyModifiers::NONE,
            });
        }
        assert!(app.note_composer.as_ref().unwrap().focused);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        app.handle_paste(" restored");
        assert_eq!(app.note_composer.as_ref().unwrap().body, "s restored");
        assert!(!app.options.sidebar);
    }

    #[test]
    fn draft_note_retains_large_synchronous_input_burst() {
        let mut app = ReviewApp::new(responsive_changeset(), ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        let text = "the quick brown fox jumps over the lazy dog 0123456789".repeat(3);
        for character in text.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("Draft note"));
        assert!(frame.contains(&text[..10]), "{frame}");
        assert!(frame.contains(&text[text.len() - 6..]), "{frame}");
        assert_eq!(app.note_composer.as_ref().unwrap().body, text);
    }

    #[test]
    fn draft_note_wraps_chunked_cjk_input_without_losing_either_end() {
        let mut app = ReviewApp::new(responsive_changeset(), ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        let body =
            "这个包主要是为了在普通的chatmodel外面包一层,在外层把toolcallid统一转换,方便后续处理";
        let chars = body.chars().collect::<Vec<_>>();
        for chunk in chars.chunks(12) {
            for character in chunk {
                app.handle_key(KeyEvent::new(KeyCode::Char(*character), KeyModifiers::NONE));
            }
            rendered_review_frame(&mut terminal, &app);
        }
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("Draft note"));
        assert!(
            frame.contains(&chars[..10].iter().collect::<String>()),
            "{frame}"
        );
        assert!(
            frame.contains(&chars[chars.len() - 4..].iter().collect::<String>()),
            "{frame}"
        );
        assert_eq!(app.note_composer.as_ref().unwrap().body, body);
    }

    #[test]
    fn draft_note_focus_accepts_sidebar_shortcut_as_text_without_toggling_sidebar() {
        let mut app = ReviewApp::new(responsive_changeset(), ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(240, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        let before = rendered_review_frame(&mut terminal, &app);
        assert!(before.contains("Draft note"));
        let beta_count = before.matches("beta.ts").count();
        assert!(beta_count > 1, "{before}");
        let sidebar = app.options.sidebar;
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        let after = rendered_review_frame(&mut terminal, &app);
        assert!(after.contains("Draft note"));
        assert!(after.contains('s'));
        assert_eq!(after.matches("beta.ts").count(), beta_count);
        assert_eq!(app.options.sidebar, sidebar);
        assert_eq!(app.note_composer.as_ref().unwrap().body, "s");
    }

    #[test]
    fn burst_line_movement_opens_draft_at_latest_cursor_without_shifting_source_row() {
        let review = navigation_changeset(vec![(
            "scroll.ts".into(),
            numbered_exports(1, 18, 0, true),
            numbered_exports(1, 18, 100, true),
        )]);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Stack,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        let row = |frame: &str, text: &str| {
            frame
                .lines()
                .position(|line| line.contains(text))
                .unwrap_or_else(|| panic!("missing {text:?}:\n{frame}"))
        };
        let initial = rendered_review_frame(&mut terminal, &app);
        let initial_active = row(&initial, "export const line01 = 1;");
        let initial_following = row(&initial, "export const line10 = 10;");
        for _ in 0..8 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        let draft = rendered_review_frame(&mut terminal, &app);
        let active = row(&draft, "export const line09 = 9;");
        let draft_row = row(&draft, "Draft note");
        assert!(
            draft.lines().nth(draft_row).unwrap().contains("L9"),
            "{draft}"
        );
        assert_eq!(active, initial_active + 8, "{draft}");
        assert_eq!(draft_row, active + 1, "{draft}");
        assert!(row(&draft, "export const line10 = 10;") > initial_following);
    }

    #[test]
    fn transparent_background_preserves_overlay_opacity_and_diff_tints() {
        fn assert_line_background(terminal: &Terminal<TestBackend>, text: &str, color: Color) {
            assert_ne!(color, Color::Reset);
            let buffer = terminal.backend().buffer();
            assert!(
                buffer
                    .content()
                    .chunks(buffer.area.width as usize)
                    .any(|row| {
                        row.iter()
                            .map(|cell| cell.symbol())
                            .collect::<String>()
                            .contains(text)
                            && row.iter().any(|cell| cell.bg == color)
                    }),
                "missing {text:?} with background {color:?}"
            );
        }
        let mut app = ReviewApp::new(
            responsive_changeset(),
            ReviewOptions {
                transparent_background: true,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("Toggle files/filter focus"));
        assert!(frame.contains("Focus filter"));
        assert_line_background(
            &terminal,
            "Focus filter",
            ratatui_theme_color(&app.options.theme.panel),
        );
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(rendered_review_frame(&mut terminal, &app).contains("Controls help"));
        assert_line_background(
            &terminal,
            "Navigation",
            ratatui_theme_color(&app.options.theme.panel),
        );

        let app = ReviewApp::new(
            responsive_changeset(),
            ReviewOptions {
                transparent_background: true,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 60)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        assert_line_background(
            &terminal,
            "betaValue",
            ratatui_theme_color(&app.options.theme.added_bg),
        );
        assert_line_background(
            &terminal,
            "beta = 1",
            ratatui_theme_color(&app.options.theme.removed_bg),
        );
    }

    #[test]
    fn top_level_menu_navigation_wraps_from_file_to_help_and_back() {
        let mut app = ReviewApp::new(responsive_changeset(), ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(220, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        for (key, visible, hidden) in [
            (KeyCode::F(10), "Toggle files/filter focus", "Controls help"),
            (KeyCode::Left, "Controls help", "Toggle files/filter focus"),
            (KeyCode::Right, "Toggle files/filter focus", "Controls help"),
        ] {
            app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
            let frame = rendered_review_frame(&mut terminal, &app);
            assert!(frame.contains(visible), "{frame}");
            assert!(!frame.contains(hidden), "{frame}");
        }
    }

    #[test]
    fn desktop_menu_bar_renders_and_dispatches_through_the_shared_command_table() {
        let backend = TestBackend::new(220, 20);
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
    fn file_presentation_menu_and_bulk_action_use_complete_review_order() {
        let review = two_file_changeset();
        let file_ids = review
            .files
            .iter()
            .map(|file| file.runtime_id.clone())
            .collect::<Vec<_>>();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let key = install_cached_test_file_view(&app, &file_ids[0]);
        app.filter = "a.rs".into();

        let menus = app.app_menus();
        let view = menus.get(&MenuId::View).unwrap();
        assert!(view.iter().any(|entry| matches!(
            entry,
            MenuEntry::Item {
                label,
                checked: Some(true),
                ..
            } if label == "File presentation: Preview"
        )));
        assert!(view.iter().any(|entry| matches!(
            entry,
            MenuEntry::Item { label, .. }
                if label == "Apply \"Preview\" to all matching files"
        )));

        app.execute_app_menu_command("workdeck.view.applyFilePresentationToAllMatching");
        let runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            runtime.file_view_selections.get(&file_ids[0]),
            Some(key.as_str())
        );
        assert_eq!(
            runtime.file_view_selections.get(&file_ids[1]),
            Some(key.as_str())
        );
        drop(runtime);
        assert!(
            app.file_presentation_menu_projection()
                .bulk_target
                .is_none()
        );

        app.filter = "b.rs".into();
        assert!(app.file_presentation_menu_projection().entries.is_empty());
        assert_eq!(
            app.selected_extension_file_view(&file_ids[0]).as_deref(),
            Some(key.as_str())
        );
        app.reload(two_file_changeset());
        assert_eq!(
            app.selected_extension_file_view(&file_ids[1]).as_deref(),
            Some(key.as_str())
        );

        app.filter.clear();
        app.execute_app_menu_command("workdeck.view.filePresentation.raw");
        assert_eq!(app.selected_extension_file_view(&file_ids[0]), None);
        app.execute_app_menu_command(&format!("workdeck.view.filePresentation.{key}"));
        assert_eq!(
            app.selected_extension_file_view(&file_ids[0]).as_deref(),
            Some(key.as_str())
        );

        app.reload(changeset());
        app.reload(two_file_changeset());
        assert_eq!(app.selected_extension_file_view(&file_ids[1]), None);
    }

    #[test]
    fn scoped_file_view_refresh_ignores_stale_ids_but_keeps_filter_hidden_review_files() {
        let review = two_file_changeset();
        let file_ids = review
            .files
            .iter()
            .map(|file| file.runtime_id.clone())
            .collect::<Vec<_>>();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let key = install_cached_test_file_view(&app, &file_ids[0]);

        app.refresh_extension_file_view(0, "probe", "preview", Some("no-such-file"));
        assert!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .file_view_epochs
                .is_empty()
        );

        app.filter = "a.rs".into();
        assert!(!app.review_file_is_visible(
            &app.with_state(|state| state.changeset().files.clone()),
            &app.with_state(|state| state.changeset().files[1].clone())
        ));
        app.refresh_extension_file_view(0, "probe", "preview", Some(&file_ids[1]));
        let runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            workdeck_extension_host::file_view_layout_epoch(
                &runtime.file_view_epochs,
                &key,
                &file_ids[0]
            ),
            0
        );
        assert_eq!(
            workdeck_extension_host::file_view_layout_epoch(
                &runtime.file_view_epochs,
                &key,
                &file_ids[1]
            ),
            1
        );
        drop(runtime);

        app.refresh_extension_file_view(0, "probe", "not-a-view", None);
        assert_eq!(
            app.status.as_deref(),
            Some("extension probe targeted unknown file view \"not-a-view\"")
        );
        assert_eq!(
            app.selected_extension_file_view(&file_ids[0]).as_deref(),
            Some(key.as_str())
        );
    }

    #[test]
    fn draft_mask_hides_and_refuses_a_presentation_without_erasing_its_choice() {
        let changeset = changeset();
        let file_id = changeset.files[0].runtime_id.clone();
        let mut app = ReviewApp::new(changeset, ReviewOptions::default());
        let key = install_cached_test_file_view(&app, &file_id);
        app.note_composer = Some(ReviewNoteComposer {
            id: "draft".into(),
            focused: true,
            thread: None,
            kind: ReviewNoteComposerKind::Create,
            target: ReviewNoteTarget {
                file_index: 0,
                hunk_index: 0,
                side: ReviewSide::New,
                line: 1,
            },
            body: String::new(),
            cursor: 0,
        });

        assert_eq!(
            app.selected_extension_file_view(&file_id).as_deref(),
            Some(key.as_str())
        );
        assert_eq!(app.presented_extension_file_view(&file_id), None);
        assert!(
            app.prepare_extension_file_view_layouts(
                &app.with_state(|state| state.changeset().clone()),
                80
            )
            .is_empty()
        );
        let projection = app.file_presentation_menu_projection();
        assert_eq!(projection.entries.len(), 1);
        assert!(projection.bulk_target.is_none());

        app.toggle_extension_file_view(0, "probe", "preview");
        assert_eq!(
            app.status.as_deref(),
            Some(workdeck_extension_api::FILE_VIEW_DRAFT_UNAVAILABLE_REASON)
        );
        assert_eq!(
            app.selected_extension_file_view(&file_id).as_deref(),
            Some(key.as_str())
        );

        app.note_composer = None;
        assert_eq!(
            app.presented_extension_file_view(&file_id).as_deref(),
            Some(key.as_str())
        );
    }

    #[test]
    fn qualified_file_view_controls_resolve_the_live_registration_owner() {
        let local = LiveFileViewRegistration {
            extension_index: 0,
            registration_identity: 1,
            title: "Local".into(),
            view: Arc::new(RegisteredFileView {
                extension_id: "caller".into(),
                view_id: "preview".into(),
                interactive_mode: false,
            }),
        };
        let qualified = LiveFileViewRegistration {
            extension_index: 1,
            registration_identity: 2,
            title: "Qualified".into(),
            view: Arc::new(RegisteredFileView {
                extension_id: "other".into(),
                view_id: "preview".into(),
                interactive_mode: true,
            }),
        };
        let registrations = [local, qualified];
        assert_eq!(
            resolve_live_file_view(&registrations, "caller", "preview")
                .unwrap()
                .extension_index,
            0
        );
        assert_eq!(
            resolve_live_file_view(&registrations, "caller", "other:preview")
                .unwrap()
                .extension_index,
            1
        );
        assert!(resolve_live_file_view(&registrations, "caller", "missing").is_none());
    }

    #[test]
    fn file_view_mode_transition_limit_stops_recursive_native_handoffs_before_dispatch() {
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.file_view_mode_transition_depth = MAX_FILE_VIEW_MODE_TRANSITION_DEPTH;
        app.enter_file_view_mode(usize::MAX, "probe", "preview");
        assert_eq!(
            app.status.as_deref(),
            Some("Extension probe exceeded the file-view mode transition limit")
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
        assert!(rendered.contains("[x] Auto layout"));
        assert!(rendered.contains("[ ] Files pane"));
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
    fn mounted_extension_command_controls_count_navigate_clamp_and_warn() {
        let mut app = ReviewApp::new(overflowing_changeset(3), ReviewOptions::default());
        let commands = app.extension_command_availability();
        assert!(commands.is_enabled("workdeck.review.nextHunk"));
        assert!(!commands.is_enabled("probe.jump"));
        assert!(!app.execute_extension_review_command("probe.jump", 1));
        assert!(app.execute_extension_review_command("workdeck.review.nextHunk", 2));
        assert_eq!(
            app.with_state(|state| (state.selection().file_index, state.selection().hunk_index)),
            (2, Some(0))
        );
        assert!(app.execute_extension_review_command("workdeck.review.alignCurrentLineCenter", 1));

        let second_file_id = app.with_state(|state| state.changeset().files[1].runtime_id.clone());
        app.select_extension_review_hunk("probe", &second_file_id, 99);
        assert_eq!(
            app.with_state(|state| (state.selection().file_index, state.selection().hunk_index)),
            (1, Some(0))
        );

        app.select_extension_review_file("probe", "no-such-file");
        assert_eq!(
            app.with_state(|state| (state.selection().file_index, state.selection().hunk_index)),
            (1, Some(0))
        );
        assert_eq!(
            app.status.as_deref(),
            Some("Extension probe selectFile targeted unknown file id \"no-such-file\"")
        );
        let backend = TestBackend::new(140, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(
            frame.contains("Extension probe selectFile targeted unknown file id \"no-such-file\""),
            "{frame}"
        );
    }

    #[test]
    fn frozen_app_host_extension_navigation_oracle_maps_identical_pins() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/app-host-extension-navigation.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(
            oracle["source"]["baseline"]["blob"],
            oracle["source"]["stable"]["blob"]
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 10_498);
        assert_eq!(oracle["oracleRuns"]["baseline"]["expectCalls"], 5);
        assert_eq!(oracle["oracleRuns"]["stable"]["expectCalls"], 5);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 3);
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
                vec![RegisteredLineHighlighter::new(0, "search", "matches")],
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
        let navigation = app.extension_runtime_bridge.create_navigation("triage");
        let pending = PendingExtensionCommand {
            extension_index: 0,
            extension_id: "triage".into(),
            command_id: "jump".into(),
            title: "Jump".into(),
            review_generation: app.extension_command_epoch,
            navigation,
        };
        app.extension_command_epoch = app.extension_command_epoch.saturating_add(1);

        app.apply_extension_command_actions(
            &pending,
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
    fn extension_file_projection_cache_reuses_only_the_exact_document() {
        let document = Arc::new(two_file_changeset());
        let mut cache = ExtensionFileProjectionCache::default();
        let first = cache.get(Arc::clone(&document));
        assert!(!first.is_materialized());
        assert!(Arc::ptr_eq(&first, &cache.get(Arc::clone(&document))));
        let equal_replacement = Arc::new(document.as_ref().clone());
        let replacement = cache.get(Arc::clone(&equal_replacement));
        assert_eq!(first.resolve(), replacement.resolve());
        assert!(!Arc::ptr_eq(&first, &replacement));
        let mut edited = equal_replacement.as_ref().clone();
        edited.files[0].path = "updated-name.rs".into();
        let expected = project_extension_diff_file(&edited.files[0]);
        let changed = cache.get(Arc::new(edited));
        assert_eq!(changed.resolve()[0], expected);
        assert_ne!(first.resolve()[0].path, changed.resolve()[0].path);
        assert!(!Arc::ptr_eq(&replacement, &changed));
        let retained = Arc::downgrade(&document);
        drop(document);
        assert!(
            retained.upgrade().is_none(),
            "cache must not retain replaced documents"
        );
        assert_eq!(
            first.resolve().len(),
            2,
            "retained projected values remain readable independently"
        );
    }

    #[test]
    fn deferred_file_projection_survives_replacement_before_its_first_read() {
        let document = Arc::new(two_file_changeset());
        let retained = Arc::downgrade(&document);
        let expected = project_extension_diff_file(&document.files[0]);
        let mut cache = ExtensionFileProjectionCache::default();
        let old = cache.get(Arc::clone(&document));
        let mut replacement = document.as_ref().clone();
        replacement.files[0].path = "replacement.rs".into();
        let current = cache.get(Arc::new(replacement));
        drop(document);
        assert!(!old.is_materialized());
        assert!(retained.upgrade().is_some());
        assert_eq!(current.resolve()[0].path, "replacement.rs");
        assert!(!old.is_materialized());
        assert_eq!(old.resolve()[0], expected);
        assert!(retained.upgrade().is_none());
        assert_eq!(current.resolve()[0].path, "replacement.rs");
    }

    #[test]
    fn review_app_commits_extension_runtime_authority_across_selection_reload_and_registry_change()
    {
        let mut app = ReviewApp::new(two_file_changeset(), ReviewOptions::default());
        let initial_projection = app
            .extension_file_projection_cache
            .lock()
            .unwrap()
            .get(app.with_state(|state| state.changeset_snapshot()));
        assert!(!initial_projection.is_materialized());
        let controls = app.extension_runtime_bridge.command_controls();
        let command_id = controls.availability().enabled[0].clone();
        let review_controls = app.extension_runtime_bridge.create_review_controls();
        let navigation = app.extension_runtime_bridge.create_navigation("probe");
        let frozen_selection = app.extension_runtime_bridge.get_selection();
        let second_file_id = app.with_state(|state| state.changeset().files[1].runtime_id.clone());

        app.select_extension_review_file("probe", &second_file_id);

        assert_ne!(
            frozen_selection.file.as_ref().map(|file| file.id.as_str()),
            Some(second_file_id.as_str())
        );
        let resolved = navigation.select_file(&second_file_id).unwrap();
        assert!(!initial_projection.is_materialized());
        assert_eq!(
            resolved.selected_file_id.as_deref(),
            Some(second_file_id.as_str())
        );
        assert_eq!(
            app.extension_runtime_bridge
                .get_selected_file_id()
                .as_deref(),
            Some(second_file_id.as_str())
        );

        app.reload(two_file_changeset());

        assert!(controls.is_enabled(&command_id));
        assert!(review_controls.snapshot().is_none());
        assert!(!navigation.is_live());

        app.extension_registry_generation = app.extension_registry_generation.saturating_add(1);
        app.commit_extension_runtime_bridge();
        assert!(!controls.is_enabled(&command_id));
        assert!(
            app.extension_runtime_bridge
                .command_controls()
                .is_enabled(&command_id)
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
    fn file_presentation_row_failures_reach_the_warning_surface_once_per_generation() {
        let notifications = ExtensionNotificationHub::new();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                extension_notifications: Some(notifications.clone()),
                ..ReviewOptions::default()
            },
        );
        let failure = workdeck_extension_api::FileViewRowFailure {
            extension_id: "probe".into(),
            view_id: "preview".into(),
            file_id: "alpha".into(),
            file_path: "alpha.ts".into(),
            row_id: "row".into(),
            layout_generation: 7,
            message: "paint exploded".into(),
        };

        report_file_view_row_failures(
            Some(&app.file_presentation_rendering),
            Some(&notifications),
            std::slice::from_ref(&failure),
        );
        report_file_view_row_failures(
            Some(&app.file_presentation_rendering),
            Some(&notifications),
            std::slice::from_ref(&failure),
        );

        let notification = app.active_extension_notification().unwrap();
        assert_eq!(
            notification.message,
            "Extension probe file view \"preview\" row \"row\" failed rendering alpha.ts • paint exploded"
        );
        assert_eq!(notification.notification_type, ExtensionNotifyType::Warning);
        let started = Instant::now();
        app.tick_extension_notifications(started);
        app.tick_extension_notifications(started + Duration::from_millis(4_001));
        assert!(app.active_extension_notification().is_none());
    }

    #[test]
    fn extension_notifications_keep_the_message_transient_without_a_stale_status_copy() {
        let notifications = ExtensionNotificationHub::new();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                extension_notifications: Some(notifications),
                ..ReviewOptions::default()
            },
        );
        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::Notify {
                message: "careful\nnow".into(),
                notification_type: ExtensionNotifyType::Warning,
            }],
        );
        assert_eq!(app.status.as_deref(), None);
        let notification = app.active_extension_notification().unwrap();
        assert_eq!(notification.message, "carefulnow");
        assert_eq!(notification.notification_type, ExtensionNotifyType::Warning);
        let started = Instant::now();
        app.tick_extension_notifications(started);
        app.tick_extension_notifications(started + Duration::from_millis(4_001));
        assert!(app.active_extension_notification().is_none());
        assert!(app.status.is_none());
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
        assert_eq!(
            request.line_cursor.unwrap().target,
            EditorLineTarget {
                side: ReviewSide::Old,
                line: 1,
            }
        );
        assert!(app.take_editor_request().is_none());

        app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        assert_eq!(
            app.status.as_deref(),
            Some("source expansion is unavailable for this file")
        );
        assert!(app.take_editor_request().is_none());
    }

    #[test]
    fn editor_shortcut_tracks_the_current_source_line_instead_of_the_hunk_start() {
        let review = parse_patch(
            "diff --git a/sample.ts b/sample.ts\n--- a/sample.ts\n+++ b/sample.ts\n@@ -1,5 +1,5 @@\n const alpha = 1;\n-const beta = 2;\n+const beta = 22222;\n const gamma = 3;\n const delta = 4;\n const epsilon = 5;\n",
            "test",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                repo: Some(PathBuf::from("/repo")),
                layout: LayoutMode::Stack,
                ..ReviewOptions::default()
            },
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        let changed_line = app.take_editor_request().unwrap().line_cursor.unwrap();
        assert_eq!(changed_line.target.side, ReviewSide::New);
        assert_eq!(changed_line.target.line, 2);

        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        let context_line = app.take_editor_request().unwrap().line_cursor.unwrap();
        assert_eq!(context_line.target.side, ReviewSide::New);
        assert_eq!(context_line.target.line, 3);
    }

    #[test]
    fn review_help_overlay_renders_the_command_derived_sections_and_rows() {
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        terminal
            .draw(|frame| {
                render_help(
                    frame.area(),
                    frame.buffer_mut(),
                    &default_help_commands(),
                    &theme,
                    0,
                );
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
    fn agent_menu_action_opens_copyable_guidance_and_modal_owns_input() {
        let backend = TestBackend::new(120, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.set_clipboard_copy_supported(true);
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert!(rendered_review_frame(&mut terminal, &app).contains("Toggle files/filter focus"));
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        }
        let menu = rendered_review_frame(&mut terminal, &app);
        assert!(menu.contains("Agent skill"), "{menu}");
        assert!(menu.contains("Next annotated file"), "{menu}");
        assert!(AGENT_SKILL_PROMPT.contains(AGENT_SKILL_COMMAND));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.show_agent_skill);
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
        for expected in [
            "Agent skill",
            "Teach your agent how to review this Workdeck session.",
            AGENT_SKILL_PROMPT_ROWS[0],
            AGENT_SKILL_COMMAND,
            "Copy prompt",
        ] {
            assert!(rendered.contains(expected), "missing {expected:?}");
        }
        let hits = app.agent_skill_dialog_hits.get().expect("agent skill hits");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: hits.copy_button.x,
            row: hits.copy_button.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            app.take_clipboard_copy_request().as_deref(),
            Some(AGENT_SKILL_PROMPT.as_str())
        );
        assert_eq!(
            app.status.as_deref(),
            Some("Copied agent skill prompt to clipboard")
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.show_agent_skill);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.show_agent_skill);
        assert!(app.agent_skill_dialog_hits.get().is_none());

        app.set_clipboard_copy_supported(false);
        app.apply_builtin_command_action(AppCommandAction::OpenAgentSkill);
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let hits = app.agent_skill_dialog_hits.get().expect("agent skill hits");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: hits.copy_button.x,
            row: hits.copy_button.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.take_clipboard_copy_request().is_none());
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.show_agent_skill);
    }

    #[test]
    fn help_modal_owns_backdrop_close_and_bounded_body_scrolling() {
        let backend = TestBackend::new(76, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let hits = app.help_dialog_hits.get().expect("help hits");
        assert!(hits.max_scroll > 0);

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: hits.content.x,
            row: hits.content.y,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.help_scroll, 1);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: hits.frame.x,
            row: hits.frame.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.show_help);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.show_help);
        assert!(app.help_dialog_hits.get().is_none());

        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(app.help_scroll, 0);
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let close = app
            .help_dialog_hits
            .get()
            .and_then(|hits| hits.close)
            .expect("close hit");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: close.x,
            row: close.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.show_help);
    }

    #[test]
    fn built_in_files_pane_uses_registered_responsive_widths() {
        let width = |total_width| {
            let plan = plan_extension_panes(
                &[ExtensionPaneSpec {
                    key: WORKDECK_FILES_PANE_KEY.into(),
                    pane: bundled_files_pane().clone(),
                }],
                &BTreeSet::from([WORKDECK_FILES_PANE_KEY.into()]),
                &BTreeMap::new(),
                Rect::new(0, 0, total_width, 20),
                48,
                MIN_EXTENSION_REVIEW_HEIGHT,
            );
            plan.panes.first().map_or(0, |pane| pane.bounds.width)
        };
        assert_eq!(width(40), 0);
        assert_eq!(width(100), 22);
        assert_eq!(width(220), 35);
        assert_eq!(width(500), 56);
    }

    #[test]
    fn mounted_sidebar_visibility_preserves_auto_forced_and_hidden_policies() {
        let app = |visibility| {
            ReviewApp::new(
                sidebar_visibility_changeset(),
                ReviewOptions {
                    layout: LayoutMode::Split,
                    sidebar_visibility: visibility,
                    sidebar: visibility != SidebarVisibility::Hidden,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            )
        };
        let render_sidebar = |width, app: &ReviewApp| {
            let backend = TestBackend::new(width, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            rendered_review_text(&mut terminal, app);
            let bounds = app.sidebar_bounds.get();
            let mut text = String::new();
            if let Some(bounds) = bounds {
                for y in bounds.y..bounds.bottom() {
                    for x in bounds.x..bounds.right() {
                        text.push_str(terminal.backend().buffer().cell((x, y)).unwrap().symbol());
                    }
                }
            }
            (bounds.is_some(), text)
        };

        let automatic = app(SidebarVisibility::Auto);
        let (visible, text) = render_sidebar(240, &automatic);
        assert!(visible);
        assert!(!text.contains("src/ui/"));
        let (visible, text) = render_sidebar(180, &automatic);
        assert!(visible);
        assert!(text.contains("src/ui/"), "sidebar frame: {text:?}");
        assert!(!render_sidebar(120, &automatic).0);

        let forced = app(SidebarVisibility::Visible);
        assert!(render_sidebar(120, &forced).0);

        let mut hidden = app(SidebarVisibility::Hidden);
        assert!(!render_sidebar(240, &hidden).0);
        hidden.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(render_sidebar(240, &hidden).0);

        let mut automatic = app(SidebarVisibility::Auto);
        assert!(!render_sidebar(120, &automatic).0);
        automatic.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(render_sidebar(120, &automatic).0);
        automatic.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(!render_sidebar(120, &automatic).0);
    }

    #[test]
    fn mounted_sidebar_resize_tracks_responsive_geometry_until_drag_override() {
        let mut app = ReviewApp::new(
            sidebar_resize_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let render_width = |width, app: &ReviewApp| {
            let backend = TestBackend::new(width, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            rendered_review_text(&mut terminal, app);
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .layout
                .panes
                .iter()
                .find(|pane| pane.key == WORKDECK_FILES_PANE_KEY)
                .cloned()
                .expect("mounted files pane")
        };

        for (width, expected_divider) in [(240, 39), (300, 49), (220, 36), (360, 57)] {
            let planned = render_width(width, &app);
            assert_eq!(planned.divider.unwrap().x, expected_divider);
        }

        let planned = render_width(240, &app);
        let divider = planned.divider.unwrap();
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: divider.x,
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: divider.x.saturating_add(30),
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: divider.x.saturating_add(30),
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        let dragged = render_width(240, &app);
        assert!(dragged.divider.unwrap().x > divider.x);
        assert_eq!(dragged.bounds.width, 56);
    }

    #[test]
    fn mounted_sidebar_resize_switches_projection_clamps_and_ignores_non_drags() {
        let mut app = ReviewApp::new(
            sidebar_resize_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let backend = TestBackend::new(240, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        rendered_review_text(&mut terminal, &app);
        let initial = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .layout
            .panes
            .iter()
            .find(|pane| pane.key == WORKDECK_FILES_PANE_KEY)
            .cloned()
            .unwrap();
        let divider = initial.divider.unwrap();
        let sidebar_text = |terminal: &Terminal<TestBackend>, divider_x| {
            let buffer = terminal.backend().buffer();
            let mut text = String::new();
            for y in buffer.area.y..buffer.area.bottom() {
                for x in buffer.area.x..divider_x {
                    text.push_str(buffer.cell((x, y)).unwrap().symbol());
                }
                text.push('\n');
            }
            text
        };
        assert!(!sidebar_text(&terminal, divider.x).contains("src/ui/"));

        for (kind, column) in [
            (MouseEventKind::Down(MouseButton::Left), divider.x),
            (MouseEventKind::Drag(MouseButton::Left), 34),
            (MouseEventKind::Up(MouseButton::Left), 34),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column,
                row: 10,
                modifiers: KeyModifiers::NONE,
            });
        }
        rendered_review_text(&mut terminal, &app);
        assert_eq!(
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .layout
                .panes
                .iter()
                .find(|pane| pane.key == WORKDECK_FILES_PANE_KEY)
                .unwrap()
                .bounds
                .width,
            33
        );
        let compact = sidebar_text(&terminal, 34);
        assert!(compact.contains("src/ui/"), "sidebar frame: {compact:?}");

        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .size_overrides
            .remove(WORKDECK_FILES_PANE_KEY);
        rendered_review_text(&mut terminal, &app);
        for (kind, column) in [
            (MouseEventKind::Down(MouseButton::Left), divider.x),
            (MouseEventKind::Drag(MouseButton::Left), 2),
            (MouseEventKind::Up(MouseButton::Left), 2),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column,
                row: 10,
                modifiers: KeyModifiers::NONE,
            });
        }
        rendered_review_text(&mut terminal, &app);
        let clamped = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .layout
            .panes
            .iter()
            .find(|pane| pane.key == WORKDECK_FILES_PANE_KEY)
            .cloned()
            .unwrap();
        assert_eq!(clamped.bounds.width, 22);
        assert_eq!(clamped.divider.unwrap().x, 23);

        app.extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .size_overrides
            .remove(WORKDECK_FILES_PANE_KEY);
        rendered_review_text(&mut terminal, &app);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: divider.x.saturating_add(40),
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: divider.x,
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Right),
            column: divider.x.saturating_add(30),
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Right),
            column: divider.x.saturating_add(30),
            row: 10,
            modifiers: KeyModifiers::NONE,
        });
        let runtime = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!runtime.resize.is_some());
        assert!(!runtime.size_overrides.contains_key(WORKDECK_FILES_PANE_KEY));
    }

    #[test]
    fn mounted_horizontal_extension_resize_uses_row_axis_and_body_gutter() {
        let pane = ExtensionPaneView {
            extension_id: "resize-test".into(),
            pane: workdeck_extension_api::PaneRegistration {
                id: "top".into(),
                title: "Top".into(),
                placement: PanePlacement::Top,
                default_open: true,
                preferred_size: None,
                width: None,
                height: Some(workdeck_extension_api::ExtensionPaneSize {
                    preferred: 4,
                    min: Some(2),
                    max: Some(8),
                    fraction: None,
                }),
                replaces: None,
                current_line: false,
                available: false,
            },
            content: ViewNode::Text {
                text: "TOP PANE".into(),
                style: ViewStyle::default(),
            },
        };
        let mut app = ReviewApp::new(
            sidebar_resize_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                sidebar_visibility: SidebarVisibility::Hidden,
                sidebar: false,
                highlight: false,
                extension_panes: vec![pane],
                ..ReviewOptions::default()
            },
        );
        let backend = TestBackend::new(240, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        rendered_review_text(&mut terminal, &app);
        let planned = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .layout
            .panes[0]
            .clone();
        assert_eq!(planned.bounds, Rect::new(1, 1, 238, 4));
        assert_eq!(planned.divider.unwrap().y, 5);

        for (kind, row) in [
            (MouseEventKind::Down(MouseButton::Left), 5),
            (MouseEventKind::Drag(MouseButton::Left), 8),
            (MouseEventKind::Up(MouseButton::Left), 8),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column: 120,
                row,
                modifiers: KeyModifiers::NONE,
            });
        }
        rendered_review_text(&mut terminal, &app);
        let planned = app
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .layout
            .panes[0]
            .clone();
        assert_eq!(planned.bounds, Rect::new(1, 1, 238, 7));
    }

    #[test]
    fn mounted_responsive_review_switches_sidebar_and_layout_at_exact_live_widths() {
        let app = ReviewApp::new(
            responsive_changeset(),
            ReviewOptions {
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let render_width = |width, app: &ReviewApp| {
            let backend = TestBackend::new(width, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            let frame = rendered_review_frame(&mut terminal, app);
            let files_visible = app
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .layout
                .panes
                .iter()
                .any(|pane| pane.key == WORKDECK_FILES_PANE_KEY);
            let layout = app.with_state(|state| state.resolved_layout(app.review_width.get()));
            (frame, files_visible, layout)
        };
        let split_rails = |frame: &str| {
            frame
                .lines()
                .any(|line| line.chars().filter(|character| *character == '▌').count() >= 2)
        };

        let (ultra_wide, visible, layout) = render_width(280, &app);
        assert!(visible);
        assert_eq!(ultra_wide.matches("alpha.ts").count(), 2);
        assert_eq!(layout, LayoutMode::Split);
        assert!(!ultra_wide.contains("Changeset summary"));

        for width in [220, 160] {
            let (frame, visible, layout) = render_width(width, &app);
            assert!(visible, "files pane hidden at width {width}");
            assert_eq!(frame.matches("alpha.ts").count(), 2);
            assert_eq!(layout, LayoutMode::Split);
            assert!(split_rails(&frame), "no split rails at width {width}");
            assert!(!frame.contains("Changeset summary"));
        }
        let (narrow, visible, layout) = render_width(159, &app);
        assert!(!visible);
        assert_eq!(narrow.matches("alpha.ts").count(), 1);
        assert_eq!(layout, LayoutMode::Split);
        assert!(split_rails(&narrow));
        assert!(!narrow.contains("Changeset summary"));

        let (tight, visible, layout) = render_width(119, &app);
        assert!(!visible);
        assert_eq!(tight.matches("alpha.ts").count(), 1);
        assert_eq!(layout, LayoutMode::Stack);
        assert!(!split_rails(&tight));
        assert!(!tight.contains("Changeset summary"));
    }

    #[test]
    fn mounted_responsive_header_keeps_stats_and_uses_three_dot_overflow() {
        let mut review = changeset();
        review.files[0].path = "packages/visual-studio-code-vscode/extension-postgres.ts".into();
        review.files[0].key = review.files[0].path.clone();
        review.files[0].runtime_id = "narrow-header".into();
        review.refresh_review_identities();
        let app = ReviewApp::new(
            review,
            ReviewOptions {
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(
            frame.contains("packages/visual-studio-cod... +1 -1"),
            "{frame}"
        );
        assert!(!frame.contains("packages/visual-studio-code-."));
    }

    #[test]
    fn mounted_responsive_files_menu_and_shortcut_follow_actual_visibility() {
        let mut medium = ReviewApp::new(
            responsive_changeset(),
            ReviewOptions {
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let backend = TestBackend::new(180, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        assert_eq!(
            rendered_review_frame(&mut terminal, &medium)
                .matches("alpha.ts")
                .count(),
            2
        );
        medium.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        medium.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        let menu = rendered_review_frame(&mut terminal, &medium);
        assert!(menu.contains("[x] Files pane"), "{menu}");
        assert!(!menu.contains("[ ] Files pane"));

        let mut tight = ReviewApp::new(
            responsive_changeset(),
            ReviewOptions {
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        assert_eq!(
            rendered_review_frame(&mut terminal, &tight)
                .matches("alpha.ts")
                .count(),
            1
        );
        tight.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(
            rendered_review_frame(&mut terminal, &tight)
                .matches("alpha.ts")
                .count(),
            2
        );
    }

    #[test]
    fn mounted_explicit_and_pager_layouts_preserve_responsive_contract() {
        let split_rails = |frame: &str| {
            frame
                .lines()
                .any(|line| line.chars().filter(|character| *character == '▌').count() >= 2)
        };
        let capture = |width, options| {
            let app = ReviewApp::new(responsive_changeset(), options);
            let backend = TestBackend::new(width, 24);
            let mut terminal = Terminal::new(backend).unwrap();
            rendered_review_frame(&mut terminal, &app)
        };

        let forced_split = capture(
            140,
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        assert_eq!(forced_split.matches("alpha.ts").count(), 1);
        assert!(!forced_split.contains("Changeset summary"));
        assert!(split_rails(&forced_split));

        let forced_stack = capture(
            240,
            ReviewOptions {
                layout: LayoutMode::Stack,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        assert_eq!(forced_stack.matches("alpha.ts").count(), 2);
        assert!(!forced_stack.contains("Changeset summary"));
        assert!(!split_rails(&forced_stack));

        for (width, split) in [(220, true), (150, true), (110, false)] {
            let frame = capture(
                width,
                ReviewOptions {
                    pager: true,
                    show_menu_bar: false,
                    sidebar_visibility: SidebarVisibility::Hidden,
                    sidebar: false,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            );
            assert!(!frame.contains("File  View  Navigate  Agent  Help"));
            assert!(!frame.contains("F10 menu"));
            assert_eq!(frame.matches("alpha.ts").count(), 1);
            assert_eq!(split_rails(&frame), split, "pager width {width}");
        }
    }

    #[test]
    fn mounted_filter_focus_suppresses_global_quit_shortcut() {
        let mut app = ReviewApp::new(
            responsive_changeset(),
            ReviewOptions {
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let backend = TestBackend::new(240, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(!app.should_quit);
        assert_eq!(app.focus, Focus::Filter);
        assert!(frame.contains("filter:"));
        assert!(frame.contains('q'));
    }

    // Hunk MIT: test/pty/chrome.test.ts.
    mod pty_chrome {
        use super::*;

        fn press(app: &mut ReviewApp, key: KeyCode) {
            app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
        }

        fn setup(width: u16, height: u16) -> (ReviewApp, Terminal<TestBackend>) {
            let app = ReviewApp::new(
                responsive_changeset(),
                ReviewOptions {
                    layout: LayoutMode::Split,
                    highlight: true,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            rendered_review_frame(&mut terminal, &app);
            (app, terminal)
        }

        fn click(app: &mut ReviewApp, terminal: &mut Terminal<TestBackend>, label: &str) {
            let frame = rendered_review_frame(terminal, app);
            let (row, line, offset) = frame
                .lines()
                .enumerate()
                .find_map(|(row, line)| line.find(label).map(|offset| (row, line, offset)))
                .unwrap_or_else(|| panic!("missing click target {label}:\n{frame}"));
            let column = measure_text_width(&line[..offset]) as u16;
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
            ] {
                app.handle_mouse_event(MouseEvent {
                    kind,
                    column,
                    row: row as u16,
                    modifiers: KeyModifiers::NONE,
                });
            }
            rendered_review_frame(terminal, app);
        }

        #[test]
        fn tab_filter_narrows_the_live_review_stream() {
            let (mut app, mut terminal) = setup(220, 24);
            let initial = rendered_review_frame(&mut terminal, &app);
            assert!(initial.contains("export const add = true;"));
            assert!(initial.contains("betaValue"));
            press(&mut app, KeyCode::Tab);
            for ch in "beta".chars() {
                press(&mut app, KeyCode::Char(ch));
            }
            let filtered = rendered_review_frame(&mut terminal, &app);
            assert!(filtered.contains("betaValue"));
            assert!(filtered.contains("filter: beta"));
            assert!(!filtered.contains("alpha.ts"));
            assert!(!filtered.contains("export const add = true;"));
        }

        #[test]
        fn slash_filter_reaches_a_file_beyond_the_initial_viewport() {
            let mut app = ReviewApp::new(
                sidebar_jump_navigation_changeset(),
                ReviewOptions {
                    layout: LayoutMode::Split,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
            let initial = rendered_review_frame(&mut terminal, &app);
            assert!(initial.contains("alphaOnly = true"));
            assert!(initial.contains("betaValue = 2"));
            press(&mut app, KeyCode::Char('/'));
            assert!(
                rendered_review_frame(&mut terminal, &app).contains("filter: type to filter files")
            );
            for ch in "delta".chars() {
                press(&mut app, KeyCode::Char(ch));
            }
            let filtered = rendered_review_frame(&mut terminal, &app);
            assert!(filtered.contains("filter: delta"));
            assert!(filtered.contains("deltaOnly = true"));
            assert!(!filtered.contains("alphaOnly = true"));
        }

        #[test]
        fn mouse_theme_notes_and_help_menu_round_trip() {
            let mut review = watched_changeset(true, Some("Adds bonus export."));
            review.files[0].agent.as_mut().unwrap().annotations[0].rationale =
                Some("Highlights the follow-up addition for review.".into());
            let mut app = ReviewApp::new(
                review,
                ReviewOptions {
                    layout: LayoutMode::Split,
                    agent_notes: true,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(140, 20)).unwrap();
            let initial = rendered_review_frame(&mut terminal, &app);
            assert!(initial.contains("Adds bonus export."));
            assert!(initial.contains("Highlights the follow-up addition for review."));
            click(&mut app, &mut terminal, "View");
            click(&mut app, &mut terminal, "Themes…");
            assert!(rendered_review_frame(&mut terminal, &app).contains("Theme selector"));
            click(&mut app, &mut terminal, "github-light-default");
            let selected = rendered_review_frame(&mut terminal, &app);
            assert!(selected.contains("Theme: github-light-default"));
            assert!(selected.contains("Adds bonus export."));
            assert!(!selected.contains("Theme selector"));
            click(&mut app, &mut terminal, "Agent");
            assert!(rendered_review_frame(&mut terminal, &app).contains("Next annotated file"));
            click(&mut app, &mut terminal, "Agent notes");
            let hidden = rendered_review_frame(&mut terminal, &app);
            assert!(!hidden.contains("Adds bonus export."));
            assert!(!hidden.contains("Agent notes"));
            click(&mut app, &mut terminal, "Agent");
            click(&mut app, &mut terminal, "Agent notes");
            assert!(rendered_review_frame(&mut terminal, &app).contains("Adds bonus export."));
            click(&mut app, &mut terminal, "Help");
            click(&mut app, &mut terminal, "Controls help");
            let help = rendered_review_frame(&mut terminal, &app);
            assert!(help.contains("Navigation"));
            assert!(help.contains("g / Home"));
        }

        #[test]
        fn mouse_save_persists_previewed_theme_before_quit() {
            let directory = tempfile::TempDir::new().unwrap();
            let config_path = directory.path().join("workdeck/config.toml");
            let mut app = ReviewApp::new(
                multi_hunk_navigation_changeset(),
                ReviewOptions {
                    view_preferences_config_path: Some(config_path.clone()),
                    view_preferences_home_directory: Some(directory.path().to_owned()),
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
            rendered_review_frame(&mut terminal, &app);
            assert!(!config_path.exists());
            press(&mut app, KeyCode::Char('t'));
            rendered_review_frame(&mut terminal, &app);
            press(&mut app, KeyCode::Down);
            assert!(rendered_review_frame(&mut terminal, &app).contains("›  github-dark-dimmed"));
            press(&mut app, KeyCode::Enter);
            assert!(!rendered_review_frame(&mut terminal, &app).contains("Theme selector"));
            press(&mut app, KeyCode::Char('q'));
            let prompt = rendered_review_frame(&mut terminal, &app);
            for label in [
                "Save view preferences?",
                "- theme = \"github-dark-default\"",
                "+ theme = \"github-dark-dimmed\"",
                "enter/s save",
            ] {
                assert!(prompt.contains(label), "{label}:\n{prompt}");
            }
            click(&mut app, &mut terminal, "enter/s save");
            assert!(
                std::fs::read_to_string(config_path)
                    .unwrap()
                    .contains("theme = \"github-dark-dimmed\"")
            );
            assert!(!app.take_quit_requested());
            app.tick_extension_notifications(Instant::now() + POST_PERSISTENCE_QUIT_DELAY);
            assert!(app.take_quit_requested());
        }

        #[test]
        fn question_mark_opens_keyboard_controls() {
            let (mut app, mut terminal) = setup(220, 24);
            press(&mut app, KeyCode::Char('?'));
            let frame = rendered_review_frame(&mut terminal, &app);
            assert!(frame.contains("Controls help"));
            assert!(frame.contains("move line-by-line"));
        }

        #[test]
        fn rapid_theme_previews_remain_responsive_with_large_highlighted_files() {
            let review = navigation_changeset(
                (0..8)
                    .map(|index| {
                        (
                            format!("theme-preview-{index}.ts"),
                            numbered_exports(1, 150, index * 1_000, false),
                            numbered_exports(1, 150, (index + 8) * 1_000, false),
                        )
                    })
                    .collect(),
            );
            let mut app = ReviewApp::new(review, ReviewOptions::default());
            let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
            rendered_review_frame(&mut terminal, &app);
            press(&mut app, KeyCode::Char('t'));
            let initial_index = app.themes.selected_index(&app.theme_catalog());
            for _ in 0..100 {
                press(&mut app, KeyCode::Char('j'));
                let frame = rendered_review_frame(&mut terminal, &app);
                assert!(frame.contains("Theme selector"));
                std::thread::sleep(Duration::from_millis(30));
            }
            let frame = rendered_review_frame(&mut terminal, &app);
            assert!(frame.contains("Theme selector"));
            assert_ne!(
                app.themes.selected_index(&app.theme_catalog()),
                initial_index
            );
            assert!(frame.lines().any(|line| line.contains("›  ")));
            assert!(app.themes.selector_open);
        }

        #[test]
        fn mouse_menu_switches_to_stacked_layout() {
            let (mut app, mut terminal) = setup(220, 24);
            assert!(
                rendered_review_frame(&mut terminal, &app)
                    .lines()
                    .any(|line| line.matches('▌').count() >= 2)
            );
            click(&mut app, &mut terminal, "View");
            let menu = rendered_review_frame(&mut terminal, &app);
            assert!(menu.contains("Stacked view"));
            assert!(menu.contains("Split view"));
            click(&mut app, &mut terminal, "Stacked view");
            let stacked = rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.layout(), LayoutMode::Stack);
            assert!(!stacked.lines().any(|line| line.matches('▌').count() >= 2));
            assert!(
                stacked.contains("1   -  export const alpha = 1;"),
                "{stacked}"
            );
            assert!(stacked.contains("1   -  export const beta = 1;"));
        }

        #[test]
        fn keyboard_menu_switches_to_stacked_layout() {
            let (mut app, mut terminal) = setup(220, 24);
            press(&mut app, KeyCode::F(10));
            let menu = rendered_review_frame(&mut terminal, &app);
            for label in ["Toggle files/filter focus", "Quit", "Reload"] {
                assert!(menu.contains(label), "{label}");
            }
            press(&mut app, KeyCode::Right);
            let menu = rendered_review_frame(&mut terminal, &app);
            for label in ["Split view", "Stacked view", "Auto layout"] {
                assert!(menu.contains(label), "{label}");
            }
            press(&mut app, KeyCode::Down);
            press(&mut app, KeyCode::Enter);
            let stacked = rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.layout(), LayoutMode::Stack);
            assert!(!stacked.lines().any(|line| line.matches('▌').count() >= 2));
            assert!(stacked.contains("1   -  export const alpha = 1;"));
        }

        #[test]
        fn menu_bar_and_body_share_one_column_outer_gutters() {
            let (app, mut terminal) = setup(100, 20);
            app.with_state(|state| state.set_layout(LayoutMode::Auto));
            let frame = rendered_review_frame(&mut terminal, &app);
            let body_row = frame
                .lines()
                .position(|line| line.contains("export const alpha"))
                .unwrap() as u16;
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(0, 0)].bg, buffer[(0, body_row)].bg);
            assert_eq!(buffer[(99, 0)].bg, buffer[(99, body_row)].bg);
            assert_ne!(buffer[(1, 0)].bg, buffer[(0, 0)].bg);
            assert_ne!(buffer[(98, 0)].bg, buffer[(99, 0)].bg);
        }
    }

    // Hunk MIT: test/pty/key-routing.test.ts, baseline 2c00f435.
    mod pty_key_routing {
        use super::*;

        fn press(app: &mut ReviewApp, key: KeyCode) {
            app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
        }

        fn setup(pager: bool, width: u16) -> (ReviewApp, Terminal<TestBackend>) {
            let review = if pager {
                navigation_changeset(vec![(
                    "scroll.ts".into(),
                    numbered_exports(1, 60, 0, true),
                    numbered_exports(1, 60, 100, true),
                )])
            } else {
                responsive_changeset()
            };
            let app = ReviewApp::new(
                review,
                ReviewOptions {
                    layout: LayoutMode::Split,
                    pager,
                    cursor_line: CursorLineMode::Off,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            rendered_review_frame(&mut terminal, &app);
            (app, terminal)
        }

        fn type_filter(app: &mut ReviewApp) {
            press(app, KeyCode::Char('/'));
            for ch in "beta".chars() {
                press(app, KeyCode::Char(ch));
            }
        }

        #[test]
        fn escape_closes_help_without_erasing_filter() {
            let (mut app, mut terminal) = setup(false, 220);
            press(&mut app, KeyCode::Char('?'));
            assert!(rendered_review_frame(&mut terminal, &app).contains("Controls help"));
            type_filter(&mut app);
            assert!(rendered_review_frame(&mut terminal, &app).contains("filter: beta"));
            press(&mut app, KeyCode::Esc);
            let frame = rendered_review_frame(&mut terminal, &app);
            assert!(!frame.contains("Controls help"));
            assert!(frame.contains("filter: beta"));
            assert!(!frame.contains("filter: type to filter files"));
            assert_eq!(app.focus, Focus::Filter);
        }

        #[test]
        fn enter_runs_menu_item_without_submitting_filter() {
            let (mut app, mut terminal) = setup(false, 220);
            type_filter(&mut app);
            let filtered = rendered_review_frame(&mut terminal, &app);
            assert!(filtered.contains("filter: beta"));
            assert!(!filtered.contains("alpha.ts"));
            for key in [
                KeyCode::F(10),
                KeyCode::Right,
                KeyCode::Down,
                KeyCode::Enter,
            ] {
                press(&mut app, key);
            }
            let frame = rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.layout(), LayoutMode::Stack);
            assert!(frame.contains("export const beta = 1;"));
            assert!(frame.contains("filter: beta"));
            assert!(!frame.contains("filter=beta"));
            assert_eq!(app.focus, Focus::Filter);
        }

        #[test]
        fn note_draft_owns_f10_and_keeps_accepting_text() {
            let (mut app, mut terminal) = setup(false, 120);
            press(&mut app, KeyCode::Char('c'));
            assert!(rendered_review_frame(&mut terminal, &app).contains("Draft note"));
            press(&mut app, KeyCode::F(10));
            let frame = rendered_review_frame(&mut terminal, &app);
            assert!(frame.contains("Draft note"));
            assert!(!frame.contains("Reload"));
            for ch in "menu stays closed".chars() {
                press(&mut app, KeyCode::Char(ch));
            }
            assert!(rendered_review_frame(&mut terminal, &app).contains("menu stays closed"));
        }

        #[test]
        fn menu_arrows_do_not_scroll_pager_behind_menu() {
            let (mut app, mut terminal) = setup(true, 140);
            press(&mut app, KeyCode::F(10));
            let before = rendered_review_frame(&mut terminal, &app);
            assert!(before.contains("Quit"));
            let anchor = before.lines().nth(20).unwrap().to_owned();
            assert!(!anchor.trim().is_empty());
            let scroll = app.scroll;
            for _ in 0..3 {
                press(&mut app, KeyCode::Down);
                rendered_review_frame(&mut terminal, &app);
            }
            let after = rendered_review_frame(&mut terminal, &app);
            assert_eq!(after.lines().nth(20).unwrap(), anchor);
            assert_eq!(app.scroll, scroll);
        }

        #[test]
        fn theme_vertical_keys_change_selection_without_scrolling_pager() {
            let (mut app, mut terminal) = setup(true, 140);
            press(&mut app, KeyCode::Char('t'));
            let before = rendered_review_frame(&mut terminal, &app);
            assert!(before.contains("Theme selector"));
            let anchor = before.lines().nth(20).unwrap().to_owned();
            assert!(!anchor.trim().is_empty());
            let selected = app.themes.selected_index(&app.theme_catalog());
            let scroll = app.scroll;
            press(&mut app, KeyCode::Char('j'));
            let after = rendered_review_frame(&mut terminal, &app);
            assert!(after.contains("Theme selector"));
            assert_eq!(after.lines().nth(20).unwrap(), anchor);
            assert_eq!(app.scroll, scroll);
            assert_ne!(app.themes.selected_index(&app.theme_catalog()), selected);
        }

        #[test]
        fn menu_accelerator_opens_help_and_closes_menu() {
            let (mut app, mut terminal) = setup(false, 120);
            press(&mut app, KeyCode::F(10));
            assert!(rendered_review_frame(&mut terminal, &app).contains("Reload"));
            press(&mut app, KeyCode::Char('?'));
            let after = rendered_review_frame(&mut terminal, &app);
            assert!(after.contains("Controls help"));
            assert!(!after.contains("Reload"));
        }

        #[test]
        fn vertical_review_key_moves_menu_without_scrolling_stream() {
            let (mut app, mut terminal) = setup(true, 120);
            app.options.pager = false;
            rendered_review_frame(&mut terminal, &app);
            press(&mut app, KeyCode::F(10));
            let before = rendered_review_frame(&mut terminal, &app);
            assert!(before.contains("Reload"));
            let anchor = before.lines().nth(20).unwrap().to_owned();
            assert!(!anchor.trim().is_empty());
            let scroll = app.scroll;
            let menus = app.app_menus();
            let selected = app
                .extension_pane_runtime
                .lock()
                .unwrap()
                .menu
                .selected_index(&menus);
            press(&mut app, KeyCode::Char('j'));
            let after = rendered_review_frame(&mut terminal, &app);
            assert_eq!(after.lines().nth(20).unwrap(), anchor);
            assert_eq!(app.scroll, scroll);
            assert_ne!(
                app.extension_pane_runtime
                    .lock()
                    .unwrap()
                    .menu
                    .selected_index(&menus),
                selected,
                "menu selection did not move"
            );
        }
    }

    // Hunk MIT: test/pty/cursor-line.test.ts; lens/mouse cases live in examples/tests/current_line_lens.rs.
    mod pty_cursor_line {
        use super::*;

        fn press(app: &mut ReviewApp, key: char) {
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
        }

        fn setup(gap: bool) -> (ReviewApp, Terminal<TestBackend>) {
            let mut review = if gap {
                parse_patch("diff --git a/gap.ts b/gap.ts\n--- a/gap.ts\n+++ b/gap.ts\n@@ -4 +4 @@\n-old\n+new\n", "cursor-gap", "Gap", ChangesetSource::WorkingTree { staged: false }).unwrap()
            } else {
                navigation_changeset(vec![(
                    "scroll.ts".into(),
                    numbered_exports(1, 60, 0, true),
                    numbered_exports(1, 60, 100, true),
                )])
            };
            if gap {
                review.files[0].set_sources(FileSourceSnapshots {
                    old: Some(SourceSnapshot::new(
                        "hiddenLine01\nhiddenLine02\nhiddenLine03\nold\n".into(),
                        SourceOrigin::Revision {
                            revision: "HEAD".into(),
                        },
                        true,
                    )),
                    new: Some(SourceSnapshot::new(
                        "hiddenLine01\nhiddenLine02\nhiddenLine03\nnew\n".into(),
                        SourceOrigin::WorkingTree,
                        false,
                    )),
                });
                review.files[0].flags.partial = false;
                review.refresh_review_identities();
            }
            let app = ReviewApp::new(
                review,
                ReviewOptions {
                    layout: LayoutMode::Stack,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(
                if gap { 140 } else { 120 },
                if gap { 16 } else { 24 },
            ))
            .unwrap();
            rendered_review_frame(&mut terminal, &app);
            (app, terminal)
        }

        fn row(frame: &str, needle: &str) -> usize {
            frame
                .lines()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("missing {needle}: {frame}"))
        }

        #[test]
        fn stepping_moves_cursor_before_viewport_and_then_one_row_per_press() {
            let (mut app, mut terminal) = setup(false);
            press(&mut app, 'j');
            rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.scroll, 0);
            let mut steps = 1;
            while app.scroll == 0 && steps < 40 {
                press(&mut app, 'j');
                rendered_review_frame(&mut terminal, &app);
                steps += 1;
            }
            assert!(steps > 5 && app.scroll > 0);
            for _ in 0..2 {
                let before = app.scroll;
                press(&mut app, 'j');
                rendered_review_frame(&mut terminal, &app);
                assert_eq!(app.scroll, before + 1);
            }
            let before = app.scroll;
            press(&mut app, 'k');
            rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.scroll, before);
        }

        #[test]
        fn held_step_burst_advances_five_rows_without_intermediate_frames() {
            let (mut app, mut terminal) = setup(false);
            for _ in 0..40 {
                if app.scroll > 0 {
                    break;
                }
                press(&mut app, 'j');
                rendered_review_frame(&mut terminal, &app);
            }
            assert!(app.scroll > 0);
            let before = rendered_review_frame(&mut terminal, &app);
            let anchor = before.lines().nth(12).unwrap().trim();
            assert!(!anchor.is_empty());
            for _ in 0..5 {
                press(&mut app, 'j');
            }
            let after = rendered_review_frame(&mut terminal, &app);
            assert_eq!(row(&after, anchor), 7);
        }

        #[test]
        fn page_navigation_keeps_cursor_visible() {
            let (mut app, mut terminal) = setup(false);
            press(&mut app, ' ');
            rendered_review_frame(&mut terminal, &app);
            let before = app.scroll;
            press(&mut app, 'j');
            rendered_review_frame(&mut terminal, &app);
            assert!(app.scroll.abs_diff(before) <= 1);
        }

        #[test]
        fn note_after_paging_preserves_the_visible_review_anchor() {
            let (mut app, mut terminal) = setup(false);
            press(&mut app, ' ');
            let paged = rendered_review_frame(&mut terminal, &app);
            let anchor = paged.lines().nth(12).unwrap().trim();
            assert!(!anchor.is_empty());
            press(&mut app, 'c');
            let draft = rendered_review_frame(&mut terminal, &app);
            assert!(draft.contains("Draft note"));
            assert!(draft.contains(anchor));
        }

        #[test]
        fn note_anchor_follows_cursor_instead_of_hunk_start() {
            let (mut app, mut terminal) = setup(false);
            press(&mut app, 'c');
            let initial = row(&rendered_review_frame(&mut terminal, &app), "Draft note");
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            for _ in 0..4 {
                press(&mut app, 'j');
                rendered_review_frame(&mut terminal, &app);
            }
            press(&mut app, 'c');
            assert!(row(&rendered_review_frame(&mut terminal, &app), "Draft note") > initial);
        }

        #[test]
        fn expanded_gap_rows_are_reachable_by_cursor_and_note() {
            let (mut app, mut terminal) = setup(true);
            press(&mut app, 'z');
            assert!(rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
            press(&mut app, 'k');
            press(&mut app, 'c');
            let draft = rendered_review_frame(&mut terminal, &app);
            assert_eq!(row(&draft, "Draft note"), row(&draft, "hiddenLine01") + 1);
        }

        #[test]
        fn expanding_moves_cursor_into_gap_and_collapsing_restores_anchor() {
            let (mut app, mut terminal) = setup(true);
            press(&mut app, 'c');
            let target = app.note_composer.as_ref().unwrap().target;
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            press(&mut app, 'z');
            rendered_review_frame(&mut terminal, &app);
            press(&mut app, 'c');
            let draft = rendered_review_frame(&mut terminal, &app);
            assert!(draft.contains("R1 "));
            assert_eq!(row(&draft, "Draft note"), row(&draft, "hiddenLine01") + 1);
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            press(&mut app, 'z');
            assert!(!rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
            press(&mut app, 'c');
            assert_eq!(app.note_composer.as_ref().unwrap().target, target);
        }

        #[test]
        fn reload_retires_expanded_gaps_and_restore_points_for_changed_or_removed_sources() {
            for removed in [false, true] {
                let (mut app, mut terminal) = setup(true);
                let original = app.with_state(|state| state.changeset().clone());
                press(&mut app, 'z');
                assert!(rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
                assert!(!app.gap_cursor_restore.is_empty());
                let mut next = original.clone();
                if removed {
                    next.files.clear();
                } else {
                    let mut sources = next.files[0].sources.clone();
                    sources.new = Some(SourceSnapshot::new(
                        "replacement01\nreplacement02\nreplacement03\nnew\n".into(),
                        SourceOrigin::WorkingTree,
                        false,
                    ));
                    next.files[0].set_sources(sources);
                }
                app.reload(next);
                assert!(app.expanded_gaps.is_empty(), "removed={removed}");
                assert!(app.gap_cursor_restore.is_empty(), "removed={removed}");
                app.reload(original);
                assert!(!rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
            }
        }

        #[test]
        fn reload_preserves_expansion_when_only_the_runtime_file_identity_changes() {
            let (mut app, mut terminal) = setup(true);
            press(&mut app, 'z');
            let expanded = app.expanded_gaps.clone();
            let restore = app.gap_cursor_restore.clone();
            let mut replacement = app.with_state(|state| state.changeset().clone());
            replacement.files[0].runtime_id = "replacement-runtime-id".into();
            app.reload(replacement);
            assert_eq!(app.expanded_gaps, expanded);
            assert_eq!(app.gap_cursor_restore, restore);
            assert!(rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
            press(&mut app, 'z');
            assert!(!rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
        }

        #[test]
        fn collapsing_a_gap_after_file_reorder_restores_the_same_semantic_file() {
            let (mut app, mut terminal) = setup(true);
            press(&mut app, 'z');
            assert!(rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
            let restored_line = app.gap_cursor_restore.values().next().unwrap().target.line;
            press(&mut app, 'j');
            assert_eq!(app.current_review_line_cursor().unwrap().target.line, 2);
            let mut replacement = app.with_state(|state| state.changeset().clone());
            let mut other = navigation_changeset(vec![(
                "other.ts".into(),
                numbered_exports(1, 8, 0, true),
                numbered_exports(1, 8, 100, true),
            )]);
            replacement.files.insert(0, other.files.remove(0));
            app.reload(replacement);
            assert_eq!(app.with_state(|state| state.selection().file_index), 1);
            assert_eq!(
                app.current_review_line_cursor().unwrap().target.file_index,
                1
            );
            assert_eq!(app.current_review_line_cursor().unwrap().target.line, 2);
            app.toggle_source_gap();
            assert_eq!(app.with_state(|state| state.selection().file_index), 1);
            let cursor = app.current_review_line_cursor().unwrap();
            assert_eq!(cursor.target.file_index, 1);
            assert_eq!(cursor.target.line, restored_line);
        }

        #[test]
        fn visible_gap_mouse_release_toggles_without_starting_copy_selection() {
            let (mut app, mut terminal) = setup(true);
            let frame = rendered_review_frame(&mut terminal, &app);
            let hit = app.review_gap_hits.lock().unwrap()[0].bounds;
            assert!(
                frame
                    .lines()
                    .nth(usize::from(hit.y))
                    .unwrap()
                    .contains("unchanged")
            );
            let mouse = |kind| MouseEvent {
                kind,
                column: hit.x + hit.width / 2,
                row: hit.y,
                modifiers: KeyModifiers::NONE,
            };
            app.handle_mouse_event(mouse(MouseEventKind::Down(MouseButton::Left)));
            assert!(app.copy_selection_drag.is_none());
            assert!(app.expanded_gaps.is_empty());
            app.handle_mouse_event(mouse(MouseEventKind::Up(MouseButton::Left)));
            assert!(rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
            assert_eq!(app.expanded_gaps.len(), 1);
            app.handle_mouse_event(mouse(MouseEventKind::Up(MouseButton::Left)));
            assert!(!rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
            assert!(app.expanded_gaps.is_empty());
        }

        #[test]
        fn gap_mouse_hits_reject_retired_frames_and_refresh_after_reload() {
            let (mut app, mut terminal) = setup(true);
            let hit = app.review_gap_hits.lock().unwrap()[0].bounds;
            let mouse = MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: hit.x,
                row: hit.y,
                modifiers: KeyModifiers::NONE,
            };
            let mut replacement = app.with_state(|state| state.changeset().clone());
            replacement.files[0].runtime_id = "replacement".into();
            app.reload(replacement);
            app.handle_mouse_event(mouse);
            assert!(app.expanded_gaps.is_empty());
            rendered_review_frame(&mut terminal, &app);
            app.handle_mouse_event(mouse);
            assert_eq!(app.expanded_gaps.len(), 1);
            rendered_review_frame(&mut terminal, &app);
            app.scroll = usize::MAX;
            terminal.backend_mut().resize(140, 6);
            rendered_review_frame(&mut terminal, &app);
            assert!(app.review_gap_hits.lock().unwrap().is_empty());
            render_review(
                Rect::new(0, 0, 0, 0),
                &mut Buffer::empty(Rect::new(0, 0, 0, 0)),
                &app,
            );
            assert!(app.review_gap_hits.lock().unwrap().is_empty());
        }

        #[test]
        fn clicking_another_file_gap_restores_the_original_cursor_file_after_reordering() {
            let (app, _) = setup(true);
            let mut review = app.with_state(|state| state.changeset().clone());
            let mut other = review.files[0].clone();
            other.path = "other.ts".into();
            other.runtime_id = "other".into();
            review.files.push(other);
            review.refresh_review_identities();
            let original_key = review.files[0].key.clone();
            let other_key = review.files[1].key.clone();
            let mut app = ReviewApp::new(
                review,
                ReviewOptions {
                    layout: LayoutMode::Stack,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
            rendered_review_frame(&mut terminal, &app);
            let hit = app
                .review_gap_hits
                .lock()
                .unwrap()
                .iter()
                .find(|hit| hit.file_key == other_key)
                .unwrap()
                .bounds;
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: hit.x,
                row: hit.y,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(
                app.with_state(|state| state.selected_file().unwrap().key.clone()),
                other_key
            );
            let mut replacement = app.with_state(|state| state.changeset().clone());
            replacement.files.reverse();
            app.reload(replacement);
            rendered_review_frame(&mut terminal, &app);
            let hit = app
                .review_gap_hits
                .lock()
                .unwrap()
                .iter()
                .find(|hit| hit.file_key == other_key)
                .unwrap()
                .bounds;
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: hit.x,
                row: hit.y,
                modifiers: KeyModifiers::NONE,
            });
            assert_eq!(
                app.with_state(|state| state.selected_file().unwrap().key.clone()),
                original_key
            );
        }

        #[test]
        fn collapsing_a_gap_keeps_a_cursor_that_moved_outside_the_gap() {
            let before = "hiddenLine01\nhiddenLine02\nhiddenLine03\nold4\nold5\n";
            let after = "hiddenLine01\nhiddenLine02\nhiddenLine03\nnew4\nnew5\n";
            let patch = create_two_files_patch("gap.ts", before, after, 0);
            let mut review = parse_patch(
                &patch,
                "cursor-gap",
                "Gap",
                ChangesetSource::WorkingTree { staged: false },
            )
            .unwrap();
            review.files[0].set_sources(FileSourceSnapshots {
                old: Some(SourceSnapshot::new(
                    before.into(),
                    SourceOrigin::Revision {
                        revision: "HEAD".into(),
                    },
                    true,
                )),
                new: Some(SourceSnapshot::new(
                    after.into(),
                    SourceOrigin::WorkingTree,
                    false,
                )),
            });
            review.files[0].flags.partial = false;
            review.refresh_review_identities();
            let mut app = ReviewApp::new(
                review,
                ReviewOptions {
                    layout: LayoutMode::Stack,
                    highlight: false,
                    ..ReviewOptions::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(140, 20)).unwrap();
            rendered_review_frame(&mut terminal, &app);
            press(&mut app, 'z');
            for _ in 0..10 {
                let cursor = app.current_review_line_cursor().unwrap();
                if cursor.target.side == ReviewSide::New && cursor.target.line == 5 {
                    break;
                }
                press(&mut app, 'j');
            }
            let before_collapse = app.current_review_line_cursor().unwrap().target;
            assert_eq!(before_collapse.side, ReviewSide::New);
            assert_eq!(before_collapse.line, 5);
            press(&mut app, 'z');
            assert_eq!(
                app.current_review_line_cursor().unwrap().target,
                before_collapse
            );
            assert!(!rendered_review_frame(&mut terminal, &app).contains("hiddenLine01"));
        }
    }

    #[test]
    fn menu_toggle_precedes_filter_and_agent_overlay_but_not_note_or_modal_ownership() {
        fn active_menu(app: &ReviewApp) -> Option<MenuId> {
            let menus = app.app_menus();
            app.extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .menu
                .active_menu_id(&menus)
        }

        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        app.focus = Focus::Filter;
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert_eq!(active_menu(&app), Some(MenuId::File));
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert_eq!(active_menu(&app), None);

        app.show_agent_skill = true;
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert_eq!(active_menu(&app), Some(MenuId::File));
        assert!(app.show_agent_skill);
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        app.show_agent_skill = false;

        app.focus = Focus::Review;
        app.open_note_composer();
        assert!(app.note_composer.is_some());
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert_eq!(active_menu(&app), None);

        app.apply_extension_actions(
            0,
            "probe",
            vec![ExtensionHostAction::OpenConfirmDialog {
                id: "modal".into(),
                title: "Modal".into(),
                body: "Own every key".into(),
                confirm_label: "yes".into(),
                cancel_label: Some("no".into()),
            }],
        );
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.has_extension_confirm_dialog());
        assert_eq!(active_menu(&app), None);
        assert!(!app.should_quit);
    }

    #[test]
    fn frozen_use_app_keyboard_shortcuts_oracle_covers_every_source_line_and_both_pins() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/use-app-keyboard-shortcuts.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["baseline"]["blob"],
            "23327df904a5ba3a0234efc9b92da9610b8500a7"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 20_985);
        assert_eq!(oracle["source"]["baseline"]["lines"], 624);
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(oracle["source"]["stable"]["bytes"], 20_684);
        assert_eq!(oracle["source"]["stable"]["lines"], 620);

        let mut next_line = 1;
        for interval in oracle["sourceCoverage"].as_array().unwrap() {
            let lines = interval["lines"].as_array().unwrap();
            assert_eq!(lines[0].as_u64().unwrap(), next_line);
            assert!(!interval["rust"].as_array().unwrap().is_empty());
            next_line = lines[1].as_u64().unwrap() + 1;
        }
        assert_eq!(next_line, 625);
        assert!(oracle["nativeTests"].as_array().unwrap().len() >= 26);
        assert_eq!(oracle["baselineDelta"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn mounted_watch_reload_preserves_filter_theme_and_refreshes_agent_note() {
        let light = resolve_theme(
            Some(DEFAULT_LIGHT_THEME_ID),
            Some(ThemeAppearance::Light),
            &[],
        );
        let mut app = ReviewApp::new(
            watched_changeset(false, None),
            ReviewOptions {
                layout: LayoutMode::Split,
                watch: true,
                agent_notes: true,
                highlight: false,
                theme: light.clone(),
                ..ReviewOptions::default()
            },
        );
        let backend = TestBackend::new(220, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        for character in "after".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(app.focus, Focus::Filter);
        assert_eq!(app.filter, "after");

        apply_reloaded_changeset(
            &mut app,
            watched_changeset(true, Some("Watch rationale updated")),
            SessionReloadReason::Watch,
        );
        let frame = rendered_review_frame(&mut terminal, &app);
        assert_eq!(app.focus, Focus::Filter);
        assert_eq!(app.filter, "after");
        assert_eq!(app.options.theme.id, light.id);
        assert_eq!(app.options.theme.panel, light.panel);
        assert!(frame.contains("observed"), "{frame}");
        assert!(frame.contains("filter:"), "{frame}");
        assert!(frame.contains("after"), "{frame}");
        assert!(frame.contains("Watch rationale updated"), "{frame}");
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|cell| cell.bg == ratatui_theme_color(&light.panel))
        );
    }

    #[test]
    fn manual_reload_replaces_changed_content_and_invalidates_the_old_syntax_cache() {
        let initial = reload_content_changeset("first");
        let previous_file = initial.files[0].clone();
        let mut app = ReviewApp::new(
            initial,
            ReviewOptions {
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        let theme = app.options.theme.clone();
        {
            let mut highlights = app
                .highlights
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            assert!(
                highlights
                    .prefetch_highlighted_diff(&previous_file, &theme, false)
                    .is_some()
            );
            assert!(
                highlights
                    .resolve_snapshot(Some(&previous_file), &theme, None, None)
                    .is_some()
            );
        }

        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(app.take_reload_requested());
        app.reload(reload_content_changeset("second"));
        assert!(
            app.highlights
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .resolve_snapshot(Some(&previous_file), &theme, None, None)
                .is_none()
        );

        let mut terminal = Terminal::new(TestBackend::new(220, 20)).unwrap();
        let rendered = rendered_review_frame(&mut terminal, &app);
        assert!(rendered.contains("second"), "{rendered}");
        assert!(!rendered.contains("first"), "{rendered}");
    }

    fn attention_mark() -> workdeck_session::HighlightToolInput {
        workdeck_session::HighlightToolInput {
            target_session: workdeck_session::SessionSelector::default(),
            file_path: "bravo.ts".into(),
            side: ReviewSide::New,
            line: 1,
            start: 13,
            end: 18,
            tone: Some(workdeck_session::SessionLineHighlightTone::Current),
            reveal: Some(true),
        }
    }

    fn assert_bravo_mark_is_painted(
        terminal: &mut Terminal<TestBackend>,
        app: &ReviewApp,
    ) -> Color {
        rendered_review_frame(terminal, app);
        let buffer = terminal.backend().buffer();
        let marked = background_for_symbol_on_text_row(buffer, "export const bravo = 2", "b");
        let unmarked = background_for_symbol_on_text_row(buffer, "export const bravo = 2", "e");
        assert_ne!(marked, unmarked);
        marked
    }

    #[test]
    fn reload_preserves_and_clears_attention_marks_when_content_is_unchanged() {
        let initial = reload_attention_changeset(false);
        let mut app = ReviewApp::new(
            initial.clone(),
            ReviewOptions {
                sidebar: false,
                line_numbers: false,
                cursor_line: CursorLineMode::Off,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        app.session_add_agent_line_highlight(&attention_mark())
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
        let painted = assert_bravo_mark_is_painted(&mut terminal, &app);

        app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(app.take_reload_requested());
        app.reload(initial);
        assert_eq!(assert_bravo_mark_is_painted(&mut terminal, &app), painted);

        let cleared = app
            .session_clear_agent_line_highlights(Some("bravo.ts"))
            .unwrap();
        assert_eq!(cleared.removed_count, 1);
        assert_eq!(cleared.remaining_count, 0);
        rendered_review_frame(&mut terminal, &app);
        assert_eq!(
            background_for_symbol_on_text_row(
                terminal.backend().buffer(),
                "export const bravo = 2",
                "b"
            ),
            background_for_symbol_on_text_row(
                terminal.backend().buffer(),
                "export const bravo = 2",
                "e"
            )
        );
    }

    #[test]
    fn reload_rekeys_painted_attention_marks_when_file_runtime_identity_shifts() {
        let initial = reload_attention_changeset(false);
        let initial_id = initial.files[0].runtime_id.clone();
        let replacement = reload_attention_changeset(true);
        let replacement_id = replacement
            .files
            .iter()
            .find(|file| file.path == "bravo.ts")
            .unwrap()
            .runtime_id
            .clone();
        assert_ne!(initial_id, replacement_id);
        let mut app = ReviewApp::new(
            initial,
            ReviewOptions {
                sidebar: false,
                line_numbers: false,
                cursor_line: CursorLineMode::Off,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        app.session_add_agent_line_highlight(&attention_mark())
            .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let painted = assert_bravo_mark_is_painted(&mut terminal, &app);

        app.reload(replacement);
        assert!(app.agent_line_highlights.get(&initial_id).is_none());
        assert_eq!(
            app.agent_line_highlights
                .get(&replacement_id)
                .map(<[_]>::len),
            Some(1)
        );
        assert_eq!(assert_bravo_mark_is_painted(&mut terminal, &app), painted);

        let cleared = app
            .session_clear_agent_line_highlights(Some("bravo.ts"))
            .unwrap();
        assert_eq!(cleared.removed_count, 1);
        assert_eq!(cleared.remaining_count, 0);
    }

    #[test]
    fn frozen_app_host_reload_oracle_maps_both_pins_and_all_source_tests() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/app-host-reload.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 15_028);
        assert_eq!(oracle["source"]["stable"]["bytes"], 13_246);
        assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 5);
        assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 4);
        assert_eq!(oracle["oracleRuns"]["baseline"]["expectCalls"], 17);
        assert_eq!(oracle["oracleRuns"]["stable"]["expectCalls"], 15);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn frozen_app_host_responsive_oracle_maps_both_pins_and_main_delta() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/app-host-responsive.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 8_674);
        assert_eq!(oracle["source"]["stable"]["bytes"], 8_012);
        assert_eq!(oracle["oracleRuns"]["baseline"]["expectCalls"], 42);
        assert_eq!(oracle["oracleRuns"]["stable"]["expectCalls"], 35);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 7);
        assert_eq!(
            oracle["pinDelta"]["commit"],
            "15cdd7c5ef491726cf091f7b95189843fe059027"
        );
    }

    #[test]
    fn frozen_app_host_watch_oracle_maps_identical_pins_and_all_tests() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/app-host-watch.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(
            oracle["source"]["baseline"]["blob"],
            oracle["source"]["stable"]["blob"]
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 7_545);
        assert_eq!(oracle["oracleRuns"]["baseline"]["expectCalls"], 17);
        assert_eq!(oracle["oracleRuns"]["stable"]["expectCalls"], 17);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn frozen_app_host_sidebar_resize_oracle_maps_both_pins_and_main_deltas() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/app-host-sidebar-resize.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 8_249);
        assert_eq!(oracle["source"]["stable"]["bytes"], 6_771);
        assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 7);
        assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 5);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 7);
        assert_eq!(oracle["pinDeltas"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn frozen_app_host_sidebar_visibility_oracle_maps_both_pins_and_baseline_delta() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/app-host-sidebar-visibility.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 6_055);
        assert_eq!(oracle["source"]["stable"]["bytes"], 5_617);
        assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 7);
        assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 6);
        assert_eq!(
            oracle["pinDelta"]["commit"],
            "15cdd7c5ef491726cf091f7b95189843fe059027"
        );
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 7);
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
    fn frozen_scroll_oracle_accounts_for_nine_source_cases_on_both_pins() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/pty-scroll.json"))
                .unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["blob"],
            "f6c4fbbf35086cfdb7fa38886914be9b4847bd41"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 12694);
        assert_eq!(
            oracle["source"]["stable"]["blob"],
            "1da31e5c0f54782ef22507934f9349ed8e298dfd"
        );
        assert_eq!(oracle["source"]["stable"]["bytes"], 12661);
        for pin in ["baseline", "stable"] {
            assert_eq!(oracle["oracle_runs"][pin]["passed"], 9);
            assert_eq!(oracle["oracle_runs"][pin]["failed"], 0);
            assert_eq!(oracle["oracle_runs"][pin]["expect_calls"], 46);
        }
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 9);
        let mut names = BTreeSet::new();
        for mapping in mappings {
            assert!(names.insert(mapping["source_test"].as_str().unwrap()));
            for evidence in mapping["evidence"].as_array().unwrap() {
                let test = evidence["test"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("tests::")
                    .unwrap();
                assert!(include_str!("lib.rs").contains(&format!("fn {test}()")));
            }
        }
    }

    #[test]
    fn scroll_short_final_file_allows_upward_movement_after_navigation() {
        let short = |factor| {
            (1..=3)
                .map(|n| format!("export const shortLine{n} = {};\n", n * factor))
                .collect::<String>()
        };
        let review = navigation_changeset(vec![
            (
                "first.ts".into(),
                numbered_exports(1, 30, 0, false),
                numbered_exports(1, 30, 100, false),
            ),
            ("second.ts".into(), short(1), short(10)),
        ]);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                cursor_line: CursorLineMode::Off,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 10)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        let bottom = rendered_review_frame(&mut terminal, &app);
        assert!(bottom.contains("shortLine1 = 10;"), "{bottom}");
        assert!(!bottom.contains("line30 = 130"), "{bottom}");
        for _ in 0..4 {
            app.scroll_diff(-1, ScrollUnit::Step);
        }
        let moved = rendered_review_frame(&mut terminal, &app);
        assert!(moved.contains("line30 = 130"), "{moved}");
    }

    #[test]
    fn arrow_step_and_reverse_restore_collapsed_gap_beneath_pinned_header() {
        for cursor_line in [CursorLineMode::Off, ReviewOptions::default().cursor_line] {
            let before = (1..=400)
                .map(|n| format!("export const line{n:03} = {n};\n"))
                .collect::<String>();
            let after = before.replace("line366 = 366", "line366 = 9999");
            let review = navigation_changeset(vec![
                ("src/ui/components/panes/DiffPane.tsx".into(), before, after),
                (
                    "other.ts".into(),
                    "export const other = 1;\n".into(),
                    "export const other = 2;\n".into(),
                ),
            ]);
            let mut app = ReviewApp::new(
                review,
                ReviewOptions {
                    layout: LayoutMode::Split,
                    cursor_line,
                    ..Default::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(220, 10)).unwrap();
            let initial = rendered_review_frame(&mut terminal, &app);
            assert!(initial.contains("DiffPane.tsx"), "{initial}");
            assert!(initial.contains("··· 362 unchanged lines ···"), "{initial}");
            assert!(
                !initial.contains("366 - export const line366 = 366;"),
                "{initial}"
            );
            let header_count = initial.matches("DiffPane.tsx").count();
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            let advanced = rendered_review_frame(&mut terminal, &app);
            if cursor_line == CursorLineMode::Off {
                assert!(
                    advanced.contains("366 - export const line366 = 366;"),
                    "{advanced}"
                );
            } else {
                // The pinned row-cursor test's waitForFrame is non-asserting:
                // its predicate times out, then the test checks only restoration.
                // A diagnostic source capture confirms the first key moves the
                // marker to context line 364 without scrolling this viewport.
                assert_eq!(app.scroll, 0);
                assert_eq!(app.current_review_line_cursor().unwrap().target.line, 364);
                assert!(
                    !advanced.contains("366 - export const line366 = 366;"),
                    "{advanced}"
                );
            }
            app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
            let restored = rendered_review_frame(&mut terminal, &app);
            assert!(
                restored.contains("··· 362 unchanged lines ···"),
                "{restored}"
            );
            assert!(
                !restored.contains("366 - export const line366 = 366;"),
                "{restored}"
            );
            assert_eq!(restored.matches("DiffPane.tsx").count(), header_count);
        }
    }

    #[test]
    fn scroll_first_wheel_step_and_reverse_restore_collapsed_gap_under_pinned_header() {
        let before = (1..=400)
            .map(|n| format!("export const line{n:03} = {n};\n"))
            .collect::<String>();
        let after = before.replace("line366 = 366", "line366 = 9999");
        let review = navigation_changeset(vec![
            ("aaa-collapsed.ts".into(), before, after),
            (
                "zzz-other.ts".into(),
                "export const other = 1;\n".into(),
                "export const other = 2;\n".into(),
            ),
        ]);
        // This patch-only fixture has no expansion source; scrolling must retain
        // the noninteractive gap label, not advertise a source fetch callback.
        assert!(review.files[0].sources.new.is_none());
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 10)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(initial.contains("··· 362 unchanged lines ···"), "{initial}");
        assert!(
            !initial.contains("366 - export const line366 = 366;"),
            "{initial}"
        );
        let now = Instant::now();
        app.handle_mouse_at(MouseEventKind::ScrollDown, now);
        let advanced = rendered_review_frame(&mut terminal, &app);
        assert!(
            advanced.contains("366 - export const line366 = 366;"),
            "{advanced}"
        );
        app.handle_mouse_at(MouseEventKind::ScrollUp, now + Duration::from_millis(200));
        let restored = rendered_review_frame(&mut terminal, &app);
        assert!(
            restored.contains("··· 362 unchanged lines ···"),
            "{restored}"
        );
        assert!(
            !restored.contains("366 - export const line366 = 366;"),
            "{restored}"
        );
        assert_eq!(
            restored.matches("aaa-collapsed.ts").count(),
            initial.matches("aaa-collapsed.ts").count()
        );
    }

    #[test]
    fn scroll_pinned_header_handoff_keeps_the_viewport_lane_stable() {
        let mut app = ReviewApp::new(
            pinned_header_navigation_changeset(),
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 10)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert_eq!(initial.matches("first.ts").count(), 2);
        let rows = app.current_review_rows();
        let header = rows.file_header_tops[&1];
        let height = app.review_height.get();
        for top in [header - 1, header] {
            app.scroll = top;
            let frame = rendered_review_frame(&mut terminal, &app);
            assert_eq!(frame.matches("first.ts").count(), 2, "{frame}");
            assert_eq!(frame.matches("second.ts").count(), 2, "{frame}");
            assert_eq!(app.review_height.get(), height);
        }
        app.scroll = header + 1;
        let frame = rendered_review_frame(&mut terminal, &app);
        assert_eq!(frame.matches("first.ts").count(), 1, "{frame}");
        assert_eq!(frame.matches("second.ts").count(), 2, "{frame}");
        assert!(frame.contains("@@ -1,16 +1,16 @@"), "{frame}");
        app.scroll += 1;
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(!frame.contains("@@ -1,16 +1,16 @@"), "{frame}");
        assert_eq!(app.review_height.get(), height);
    }

    #[test]
    fn plain_section_cache_reuses_geometry_but_repaints_theme_and_rebuilds_on_resize() {
        let review = navigation_changeset(vec![
            ("a.rs".into(), "old 日\n".repeat(12), "new 🚀\n".repeat(12)),
            ("b.rs".into(), "old b\n".repeat(5), "new b\n".repeat(5)),
        ]);
        let mut app = ReviewApp::new(
            review.clone(),
            ReviewOptions {
                layout: LayoutMode::Split,
                wrap_lines: false,
                highlight: false,
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        let retained = Arc::clone(
            &app.review_plain_height
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .sections,
        );
        app.scroll = 1;
        app.options.theme = resolve_theme(Some("github-light"), None, &[]);
        let retained_gaps = Arc::clone(
            &app.review_plain_height
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .gap_geometries,
        );
        rendered_review_frame(&mut terminal, &app);
        assert!(Arc::ptr_eq(
            &retained_gaps,
            &app.review_plain_height
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .gap_geometries
        ));
        assert_eq!(
            *retained_gaps,
            review
                .files
                .iter()
                .map(PlainFileGeometry::new)
                .collect::<Vec<_>>()
        );
        assert!(Arc::ptr_eq(
            &retained,
            &app.review_plain_height
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .sections
        ));
        // Hold interaction history constant and compare complete renderer buffers
        // with cached geometry against a forced geometry rebuild.
        let mut actual = Buffer::empty(Rect::new(0, 0, 100, 20));
        let mut expected = Buffer::empty(actual.area);
        render(actual.area, &mut actual, &app);
        *app.review_plain_height.lock().unwrap() = None;
        render(expected.area, &mut expected, &app);
        for (index, (actual, expected)) in actual.content.iter().zip(&expected.content).enumerate()
        {
            assert_eq!(actual, expected, "frame cell {index}");
        }
        let mut resized = Terminal::new(TestBackend::new(80, 20)).unwrap();
        rendered_review_frame(&mut resized, &app);
        assert!(!Arc::ptr_eq(
            &retained_gaps,
            &app.review_plain_height
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .gap_geometries
        ));
        assert!(!Arc::ptr_eq(
            &retained,
            &app.review_plain_height
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .sections
        ));
        let mut actual = Buffer::empty(Rect::new(0, 0, 80, 20));
        let mut expected = Buffer::empty(actual.area);
        render(actual.area, &mut actual, &app);
        *app.review_plain_height.lock().unwrap() = None;
        render(expected.area, &mut expected, &app);
        for (index, (actual, expected)) in actual.content.iter().zip(&expected.content).enumerate()
        {
            assert_eq!(actual, expected, "resized frame cell {index}");
        }
    }

    #[test]
    fn borrowed_sidebar_entries_match_owned_filtered_frames_and_hits() {
        let review = navigation_changeset(vec![
            ("src/日.rs".into(), "old\n".into(), "new\n".into()),
            ("README.md".into(), "before\n".into(), "after\n".into()),
            ("src/nested/🚀.rs".into(), "a\n".into(), "b\n".into()),
        ]);
        for filter in ["", "src/", "README", "absent"] {
            let owned = review
                .files
                .iter()
                .filter(|file| diff_file_matches_filter(file, filter))
                .cloned()
                .collect::<Vec<_>>();
            for width in [0, 1, 12, 30, 80] {
                for scroll_top in [0, 1, 4, 100] {
                    let area = Rect::new(0, 0, width, 8);
                    let options = WorkdeckFileNavOptions {
                        selected_file_id: Some(review.files[2].runtime_id.clone()),
                        theme: "github-light".into(),
                    };
                    let borrowed = review
                        .files
                        .iter()
                        .filter(|file| diff_file_matches_filter(file, filter));
                    let entries = match resolve_file_sidebar_mode(width.saturating_sub(1)) {
                        FileSidebarMode::Flat => {
                            public_review::build_flat_sidebar_entries_borrowed(borrowed)
                        }
                        FileSidebarMode::Tree => {
                            public_review::build_tree_sidebar_entries_borrowed(borrowed)
                        }
                    };
                    let mut actual = Buffer::empty(area);
                    let mut expected = Buffer::empty(area);
                    let actual_map = public_review::render_file_nav_entries(
                        area,
                        &mut actual,
                        entries,
                        &options,
                        scroll_top,
                    );
                    let expected_map = render_workdeck_file_nav_window(
                        area,
                        &mut expected,
                        &owned,
                        &options,
                        scroll_top,
                    );
                    assert_eq!(actual, expected);
                    assert_eq!(actual_map, expected_map);
                }
            }
        }
    }

    #[test]
    fn plain_height_cache_invalidates_settings_and_equal_generation_replacement() {
        let review = navigation_changeset(vec![
            ("a.rs".into(), "old\n".repeat(3), "new\n".repeat(3)),
            ("b.rs".into(), "old\n".repeat(4), "new\n".repeat(4)),
        ]);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                wrap_lines: false,
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        let original = app.options.clone();
        let width = app.review_width.get();
        let registry_generation = app.extension_registry_generation;
        assert!(app.review_plain_height.lock().unwrap().is_some());
        assert_eq!(
            app.current_review_content_height(),
            app.current_review_geometry_rows().lines.len()
        );
        for case in 0..8 {
            app.options = original.clone();
            app.extension_registry_generation = registry_generation;
            app.filter.clear();
            app.review_width.set(width);
            app.with_state(|state| state.set_layout(LayoutMode::Split));
            match case {
                0 => app.options.file_gap += 1,
                1 => app.options.hunk_gap += 1,
                2 => app.options.hunk_headers = !app.options.hunk_headers,
                3 => app.options.pager = !app.options.pager,
                4 => app.review_width.set(width + 1),
                5 => app.filter = "a.rs".into(),
                6 => app.with_state(|state| state.set_layout(LayoutMode::Stack)),
                _ => app.extension_registry_generation = registry_generation.wrapping_add(1),
            }
            app.with_state(|state| {
                assert!(
                    !app.review_plain_height
                        .lock()
                        .unwrap()
                        .as_ref()
                        .unwrap()
                        .matches(
                            state,
                            &app.options,
                            app.review_width.get(),
                            &app.filter,
                            app.extension_registry_generation
                        )
                )
            });
            assert_eq!(
                app.current_review_content_height(),
                app.current_review_geometry_rows().lines.len()
            );
        }
        app.options = original;
        app.extension_registry_generation = registry_generation;
        app.filter.clear();
        app.review_width.set(width);
        let replacement = navigation_changeset(vec![(
            "replacement.rs".into(),
            "old\n".repeat(30),
            "new\n".repeat(30),
        )]);
        let mut replacement = ReviewState::new(replacement);
        replacement.set_layout(LayoutMode::Split);
        assert_eq!(
            app.with_state(|state| state.generation()),
            replacement.generation()
        );
        *app.shared_state().lock().unwrap() = replacement;
        assert_ne!(
            app.current_review_content_height(),
            app.review_plain_height
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .height
        );
        assert_eq!(
            app.current_review_content_height(),
            app.current_review_geometry_rows().lines.len()
        );
        // Notes/wrapping bypass the cache even on the same document.
        rendered_review_frame(&mut terminal, &app);
        let key = app.with_state(|state| state.changeset().files[0].key.clone());
        app.with_state(|state| {
            state
                .add_comment(saved_comment(&key, "height-note", "extra note"))
                .unwrap()
        });
        assert_eq!(
            app.current_review_content_height(),
            app.current_review_geometry_rows().lines.len()
        );
        app.options.wrap_lines = true;
        rendered_review_frame(&mut terminal, &app);
        assert!(app.review_plain_height.lock().unwrap().is_none());
    }

    #[test]
    fn selected_file_projection_matches_full_projection_for_duplicate_ids_and_missing_selection() {
        let base = navigation_changeset(vec![
            ("a.rs".into(), "old a\n".into(), "new a\n".into()),
            ("b.rs".into(), "old b\n".into(), "new b\n".into()),
        ]);
        for shared_id in [None, Some("duplicate"), Some("")] {
            let mut changeset = base.clone();
            if let Some(id) = shared_id {
                for file in &mut changeset.files {
                    file.runtime_id = id.into();
                }
            }
            let files = changeset
                .files
                .iter()
                .map(project_extension_diff_file)
                .collect::<Vec<_>>();
            for file_index in [0, 1, 2, usize::MAX] {
                for hunk_index in [None, Some(0), Some(9)] {
                    for side in [None, Some(ReviewSide::Old), Some(ReviewSide::New)] {
                        let snapshot = workdeck_core::ReviewSnapshot {
                            generation: 7,
                            changeset: changeset.clone(),
                            selection: ReviewSelection {
                                file_index,
                                hunk_index,
                                side,
                                line: Some(1),
                            },
                        };
                        let selected = changeset
                            .files
                            .get(file_index)
                            .map(|file| file.runtime_id.as_str());
                        let cursor = hunk_index.zip(side).map(|(hunk_index, side)| {
                            workdeck_extension_host::ExtensionLineCursor {
                                file_id: selected.unwrap_or_default().into(),
                                hunk_index,
                                target: workdeck_extension_api::ExtensionReviewSelectionLine {
                                    side: match side {
                                        ReviewSide::Old => {
                                            workdeck_extension_api::ExtensionFileSide::Old
                                        }
                                        ReviewSide::New => {
                                            workdeck_extension_api::ExtensionFileSide::New
                                        }
                                    },
                                    line: 1,
                                },
                            }
                        });
                        let expected = workdeck_extension_host::build_extension_review_selection(
                            &files,
                            selected,
                            hunk_index.map(|index| index as f64),
                            cursor.as_ref(),
                        );
                        assert_eq!(
                            workdeck_extension_host::build_extension_review_selection_from_snapshot(
                                &snapshot
                            ),
                            expected
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn per_file_digit_override_matches_isolated_rendering_after_large_line_numbers() {
        let patch = [("a.rs", 1), ("b.rs", 1000), ("c.rs", 9)]
            .into_iter()
            .map(|(path, line)| format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -{line} +{line} @@\n-old 日 value\n+new 🚀 value\n"
            )).collect::<String>();
        let review = parse_patch(
            &patch,
            "digits",
            "Digits",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        for layout in [LayoutMode::Split, LayoutMode::Stack] {
            for wrap_lines in [false, true] {
                for line_number_digits in [None, Some(4)] {
                    for width in [24, 80] {
                        let options = ReviewOptions {
                            layout,
                            wrap_lines,
                            line_number_digits,
                            highlight: false,
                            ..ReviewOptions::default()
                        };
                        let selection = ReviewSelection {
                            file_index: usize::MAX,
                            ..ReviewSelection::default()
                        };
                        let full = build_review_rows(
                            &review,
                            &[],
                            selection,
                            layout,
                            &options,
                            width,
                            &mut HighlightedDiffRuntime::default(),
                            &BTreeSet::new(),
                        );
                        for (index, file) in review.files.iter().enumerate() {
                            let mut isolated = review.clone();
                            isolated.files = vec![file.clone()];
                            let single = build_review_rows(
                                &isolated,
                                &[],
                                selection,
                                layout,
                                &options,
                                width,
                                &mut HighlightedDiffRuntime::default(),
                                &BTreeSet::new(),
                            );
                            let full_top = full.hunk_tops[&(index, 0)];
                            let single_top = single.hunk_tops[&(0, 0)];
                            let height = full.hunk_heights[&(index, 0)];
                            assert_eq!(height, single.hunk_heights[&(0, 0)]);
                            assert_eq!(
                                full.lines[full_top..full_top + height],
                                single.lines[single_top..single_top + height]
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn live_plain_split_prefetch_requests_halo_not_every_file_and_follows_eof_jump() {
        let mut app = ReviewApp::new(
            navigation_changeset(
                (0..40)
                    .map(|index| {
                        (
                            format!("file{index}.ts"),
                            "const old = 1;\n".repeat(15),
                            "const new = 2;\n".repeat(15),
                        )
                    })
                    .collect(),
            ),
            ReviewOptions {
                layout: LayoutMode::Split,
                wrap_lines: false,
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        let keys = app.with_state(|state| {
            state
                .changeset()
                .files
                .iter()
                .map(|file| {
                    highlighted_diff_runtime::highlighted_diff_cache_key(&app.options.theme, file)
                })
                .collect::<Vec<_>>()
        });
        let cached = |app: &ReviewApp, index: usize| {
            app.highlights
                .lock()
                .unwrap()
                .coordinator_mut()
                .peek(keys[index].as_str())
                .is_some()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
        let first = rendered_review_frame(&mut terminal, &app);
        assert!(first.contains("file0.ts"), "{first}");
        assert!(cached(&app, 0));
        assert!(cached(&app, 1));
        assert!(!cached(&app, 10));
        assert!(!cached(&app, 39));
        app.scroll = usize::MAX;
        let last = rendered_review_frame(&mut terminal, &app);
        assert!(last.contains("file39.ts"), "{last}");
        assert!(cached(&app, 39));
        assert!(!cached(&app, 10));
    }

    #[test]
    fn compact_note_targets_match_tree_order_lookup_and_shift_collisions() {
        // Every ordering/duplicate pattern up to seven entries over three rows.
        let exhaustive = (0..=7).flat_map(|length| {
            (0..3_usize.pow(length)).map(move |mut encoded| {
                (0..length)
                    .map(|_| {
                        let row = [0, 2, 8][encoded % 3];
                        encoded /= 3;
                        row
                    })
                    .collect::<Vec<_>>()
            })
        });
        for keys in [
            vec![],
            vec![0],
            vec![8, 2, 8, 4, 2, 8],
            vec![0, 1, 2, 3, usize::MAX],
        ]
        .into_iter()
        .chain(exhaustive)
        {
            let entries = keys
                .into_iter()
                .enumerate()
                .map(|(index, row)| {
                    (
                        row,
                        ReviewNoteTarget {
                            file_index: index,
                            hunk_index: index,
                            side: ReviewSide::New,
                            line: index as u32 + 1,
                        },
                    )
                })
                .collect::<Vec<_>>();
            let tree = entries.iter().copied().collect::<BTreeMap<_, _>>();
            let compact = entries.into_iter().collect::<ReviewNoteTargets>();
            assert_eq!(
                compact.iter().collect::<Vec<_>>(),
                tree.iter().collect::<Vec<_>>()
            );
            for row in [0, 1, 2, 3, 4, 7, 8, 9, usize::MAX] {
                assert_eq!(compact.get(&row), tree.get(&row));
            }
            // Composer removal can collapse multiple row addresses into one.
            for removed in [0, 1, 5, usize::MAX] {
                for inserted in [0, 2, usize::MAX] {
                    let shift = |(&row, &target): (&usize, &ReviewNoteTarget)| {
                        (row.saturating_sub(removed).saturating_add(inserted), target)
                    };
                    let shifted_tree = tree.iter().map(shift).collect::<BTreeMap<_, _>>();
                    let shifted_compact = compact.iter().map(shift).collect::<ReviewNoteTargets>();
                    assert_eq!(
                        shifted_compact.iter().collect::<Vec<_>>(),
                        shifted_tree.iter().collect::<Vec<_>>()
                    );
                }
            }
        }
    }

    #[test]
    fn viewport_split_rows_preserve_visible_styles_and_complete_geometry() {
        for width in [0, 1, 24, 80, 240] {
            for hunk_headers in [false, true] {
                let app = ReviewApp::new(
                    navigation_changeset(vec![
                        (
                            "one.ts".into(),
                            "old 日\ncontext\n".repeat(4),
                            "new 🚀\ncontext\n".repeat(4),
                        ),
                        ("two.ts".into(), "remove\n".into(), "added\nextra\n".into()),
                    ]),
                    ReviewOptions {
                        layout: LayoutMode::Split,
                        wrap_lines: false,
                        horizontal_offset: 3,
                        hunk_headers,
                        ..ReviewOptions::default()
                    },
                );
                app.review_width.set(width);
                app.prefetch_file_highlights(0);
                app.prefetch_file_highlights(1);
                let full = app.current_review_rows();
                let gaps = app
                    .state
                    .lock()
                    .unwrap()
                    .changeset()
                    .files
                    .iter()
                    .map(PlainFileGeometry::new)
                    .collect::<Vec<_>>();
                for start in [0, 1, full.lines.len() / 2, full.lines.len()] {
                    for height in [0, 1, 10] {
                        let end = start.saturating_add(height).min(full.lines.len());
                        let window = app.current_review_rows_with_options(
                            &app.options,
                            ReviewRowPurpose::Viewport {
                                start,
                                end,
                                highlight_files: None,
                                gap_geometries: Some(&gaps),
                                row_capacity: full.lines.len(),
                            },
                        );
                        assert_eq!(full.lines.len(), window.lines.len());
                        assert_eq!(full.lines[start..end], window.lines[start..end]);
                        assert_eq!(full.line_cursors, window.line_cursors);
                        assert_eq!(full.note_targets, window.note_targets);
                        assert_eq!(full.file_tops, window.file_tops);
                        assert_eq!(full.file_header_tops, window.file_header_tops);
                        assert_eq!(full.file_body_tops, window.file_body_tops);
                        assert_eq!(full.hunk_tops, window.hunk_tops);
                        assert_eq!(full.hunk_heights, window.hunk_heights);
                        for cursor in &window.line_cursors {
                            if !(start..end).contains(&cursor.row) {
                                assert!(window.lines[cursor.row].spans.is_empty());
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn wheel_geometry_matches_highlighted_rows_across_unicode_layouts_and_widths() {
        let before = "export const label = '日本語 🚀';\n\t// café 短い\n".repeat(5);
        let after =
            "export const label = '中文 ✨ and a much longer wrapped value';\n\t// café 長い\n"
                .repeat(5)
                + "// extra unpaired addition 🚀\n";
        for layout in [LayoutMode::Split, LayoutMode::Stack] {
            for wrap_lines in [false, true] {
                for width in [0, 1, 2, 8, 24, 80, 240] {
                    let app = ReviewApp::new(
                        navigation_changeset(vec![(
                            "unicode.ts".into(),
                            before.clone(),
                            after.clone(),
                        )]),
                        ReviewOptions {
                            layout,
                            wrap_lines,
                            horizontal_offset: if wrap_lines { 0 } else { 17 },
                            ..ReviewOptions::default()
                        },
                    );
                    app.with_state(|state| {
                        let key = state.changeset().files[0].key.clone();
                        let file = &state.changeset().files[0];
                        let owned = review_gap_source_for_file(file);
                        let geometry = workdeck_review::review_gap_geometry_for_file(file);
                        assert_eq!(owned.hunks, geometry.hunks);
                        assert_eq!(owned.addition_lines.len(), geometry.addition_line_count);
                        assert_eq!(owned.deletion_lines.len(), geometry.deletion_line_count);
                        for index in 0..=file.hunks.len() {
                            assert_eq!(
                                review_leading_gap(&owned, index),
                                geometry.leading_gap(index)
                            );
                        }
                        assert_eq!(review_trailing_gap(&owned), geometry.trailing_gap());
                        state
                            .add_comment(saved_comment(&key, "agent-geometry", "日本語 note"))
                            .unwrap();
                        let mut comment = saved_comment(&key, "user-geometry", "Saved user note");
                        comment.source = "user".into();
                        comment.editable = true;
                        state.add_comment(comment).unwrap();
                    });
                    app.review_width.set(width);
                    app.prefetch_file_highlights(0);
                    let highlighted = app.current_review_rows();
                    let geometry = app.current_review_geometry_rows();
                    let text = |rows: &ReviewRows| {
                        rows.lines
                            .iter()
                            .map(|line| {
                                line.spans
                                    .iter()
                                    .map(|span| span.content.as_ref())
                                    .collect::<String>()
                            })
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(highlighted.lines.len(), geometry.lines.len());
                    if wrap_lines {
                        assert_eq!(
                            text(&highlighted),
                            text(&geometry),
                            "{layout:?} wrap={wrap_lines} width={width}"
                        );
                    }
                    // Geometry-only unwrapped code deliberately carries no paint
                    // content. Copy/render still request the full painted plan.
                    let mut unhighlighted = app.options.clone();
                    unhighlighted.highlight = false;
                    let painted = app
                        .current_review_rows_with_options(&unhighlighted, ReviewRowPurpose::Paint);
                    assert_eq!(text(&highlighted), text(&painted));
                    assert_eq!(highlighted.line_cursors, geometry.line_cursors);
                    assert_eq!(highlighted.note_targets, geometry.note_targets);
                    assert_eq!(highlighted.note_bounds, geometry.note_bounds);
                    assert!(!geometry.note_bounds.is_empty());
                    assert_eq!(highlighted.file_tops, geometry.file_tops);
                    assert_eq!(highlighted.file_header_tops, geometry.file_header_tops);
                    assert_eq!(highlighted.file_body_tops, geometry.file_body_tops);
                    assert_eq!(highlighted.hunk_tops, geometry.hunk_tops);
                    assert_eq!(highlighted.hunk_heights, geometry.hunk_heights);
                }
            }
        }
    }

    #[test]
    fn scroll_wheel_at_eof_does_not_accumulate_invisible_overscroll() {
        let review = navigation_changeset(vec![(
            "after.ts".into(),
            numbered_exports(1, 18, 0, true),
            numbered_exports(1, 18, 100, true),
        )]);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        let now = Instant::now();
        for step in 0..42 {
            app.handle_mouse_at(
                MouseEventKind::ScrollDown,
                now + Duration::from_millis(step * 200),
            );
        }
        let bottom = rendered_review_frame(&mut terminal, &app);
        assert!(bottom.contains("line18 = 118"), "{bottom}");
        let scroll = app.scroll;
        app.handle_mouse_at(MouseEventKind::ScrollUp, now + Duration::from_secs(10));
        assert_eq!(app.scroll, scroll - 1);
        for step in 0..30 {
            app.handle_mouse_at(
                MouseEventKind::ScrollUp,
                now + Duration::from_secs(11) + Duration::from_millis(step * 200),
            );
        }
        let restored = rendered_review_frame(&mut terminal, &app);
        assert!(restored.contains("line01 = 101"), "{restored}");
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn scroll_live_scrollbar_track_and_drag_move_the_visible_review() {
        let review = navigation_changeset(vec![(
            "after.ts".into(),
            numbered_exports(1, 18, 0, true),
            numbered_exports(1, 18, 100, true),
        )]);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 10)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(initial.contains("line01 = 101"));
        assert!(!initial.contains("line12 = 112"));
        let now = Instant::now();
        for step in 0..5 {
            app.handle_mouse_at(
                MouseEventKind::ScrollDown,
                now + Duration::from_millis(step * 200),
            );
        }
        let moved = rendered_review_frame(&mut terminal, &app);
        assert!(
            moved.contains("line08 = 108") || moved.contains("line09 = 109"),
            "{moved}"
        );
        let map = app.review_scrollbar_hits.get().unwrap();
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: map.track.x,
            row: map.track.bottom() - 1,
            modifiers: KeyModifiers::NONE,
        });
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: map.track.x,
            row: map.track.bottom() - 1,
            modifiers: KeyModifiers::NONE,
        });
        let clicked = rendered_review_frame(&mut terminal, &app);
        assert!(
            clicked.contains("line12 = 112") || clicked.contains("line13 = 113"),
            "{clicked}"
        );
        assert!(!clicked.contains("line01 = 101"));
        let map = app.review_scrollbar_hits.get().unwrap();
        for (kind, row) in [
            (MouseEventKind::Down(MouseButton::Left), map.thumb.y),
            (
                MouseEventKind::Drag(MouseButton::Left),
                map.track.bottom() - 1,
            ),
            (
                MouseEventKind::Up(MouseButton::Left),
                map.track.bottom() - 1,
            ),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column: map.track.x,
                row,
                modifiers: KeyModifiers::NONE,
            });
        }
        let dragged = rendered_review_frame(&mut terminal, &app);
        assert!(
            dragged.contains("line15 = 115") || dragged.contains("line16 = 116"),
            "{dragged}"
        );
        assert!(!dragged.contains("line01 = 101"));
    }

    #[test]
    fn scroll_step_keys_after_a_code_click_move_exactly_one_row() {
        let mut app = ReviewApp::new(
            pinned_header_navigation_changeset(),
            ReviewOptions {
                cursor_line: CursorLineMode::Off,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        for (key, expected) in [('j', 1), ('k', 0)] {
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            assert_eq!(app.scroll, expected);
        }
        let frame = rendered_review_frame(&mut terminal, &app);
        let row = frame
            .lines()
            .position(|line| line.contains("line08"))
            .unwrap() as u16;
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            app.handle_mouse_event(MouseEvent {
                kind,
                column: 60,
                row,
                modifiers: KeyModifiers::NONE,
            });
        }
        for (key, expected) in [('j', 1), ('j', 2), ('k', 1)] {
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.scroll, expected);
        }
    }

    #[test]
    fn scroll_wheel_round_trip_moves_visible_code_and_restores_first_line() {
        let review = navigation_changeset(vec![(
            "after.ts".into(),
            numbered_exports(1, 18, 0, true),
            numbered_exports(1, 18, 100, true),
        )]);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(initial.contains("line01 = 101"));
        assert!(!initial.contains("line08 = 108"), "{initial}");
        let now = Instant::now();
        for step in 0..12 {
            app.handle_mouse_at(
                MouseEventKind::ScrollDown,
                now + Duration::from_millis(step * 200),
            );
        }
        let scrolled = rendered_review_frame(&mut terminal, &app);
        assert!(!scrolled.contains("line01 = 101"));
        assert!(
            scrolled.contains("line11 = 111") || scrolled.contains("line12 = 112"),
            "{scrolled}"
        );
        for step in 0..12 {
            app.handle_mouse_at(
                MouseEventKind::ScrollUp,
                now + Duration::from_secs(3) + Duration::from_millis(step * 200),
            );
        }
        let restored = rendered_review_frame(&mut terminal, &app);
        assert!(restored.contains("line01 = 101"), "{restored}");
    }

    #[test]
    fn arrow_keys_scroll_visible_lines_and_return_to_top_in_review_and_pager() {
        for pager in [false, true] {
            let mut review = navigation_changeset(vec![(
                "scroll.ts".into(),
                numbered_exports(1, 18, 0, true),
                numbered_exports(1, 18, 100, true),
            )]);
            review.files[0].agent = Some(AgentFileContext {
                path: "scroll.ts".into(),
                summary: Some("scroll.ts note".into()),
                annotations: vec![
                    serde_json::from_value(serde_json::json!({
                        "new_range": {"start":2,"end":2}, "summary": "Annotation for scroll.ts",
                        "rationale": "Why scroll.ts changed"
                    }))
                    .unwrap(),
                ],
            });
            review.refresh_review_identities();
            assert_fixture_annotation_range(&review, 2);
            let mut app = ReviewApp::new(
                review,
                ReviewOptions {
                    pager,
                    show_menu_bar: !pager,
                    layout: LayoutMode::Split,
                    agent_notes: false,
                    ..Default::default()
                },
            );
            let mut terminal =
                Terminal::new(TestBackend::new(220, if pager { 8 } else { 12 })).unwrap();
            let mut frame = rendered_review_frame(&mut terminal, &app);
            assert!(frame.contains("line01"), "{frame}");
            assert!(!frame.contains("line08"), "{frame}");
            for _ in 0..if pager { 32 } else { 48 } {
                app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                frame = rendered_review_frame(&mut terminal, &app);
                if frame.contains("line08") && (pager || !frame.contains("line01")) {
                    break;
                }
            }
            assert!(frame.contains("line08"), "pager={pager}\n{frame}");
            assert!(!frame.contains("line01"), "pager={pager}\n{frame}");
            for _ in 0..32 {
                app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
                frame = rendered_review_frame(&mut terminal, &app);
                if frame.contains("line01") {
                    break;
                }
            }
            assert!(frame.contains("line01"), "pager={pager}\n{frame}");
        }
    }

    #[test]
    fn wrap_toggle_preserves_first_visible_added_line_after_arrow_scrolling() {
        let before = numbered_exports(1, 18, 0, true);
        let after = numbered_exports(1, 18, 100, true)
            .lines()
            .map(|line| {
                format!(
                    "{line} // this is intentionally long wrap coverage for viewport anchoring\n"
                )
            })
            .collect::<String>();
        let mut review = navigation_changeset(vec![("wrap-scroll.ts".into(), before, after)]);
        review.files[0].agent = Some(
            serde_json::from_value(serde_json::json!({
                "path": "wrap-scroll.ts", "summary": "wrap-scroll.ts note",
            "annotations": [{"new_range": {"start":2,"end":2}, "summary": "Annotation for wrap-scroll.ts",
                    "rationale": "Why wrap-scroll.ts changed"}]
            }))
            .unwrap(),
        );
        review.refresh_review_identities();
        assert_fixture_annotation_range(&review, 2);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(102, 12)).unwrap();
        let first_added = |frame: &str| {
            frame.as_bytes().windows(12).find_map(|bytes| {
                let text = std::str::from_utf8(bytes).ok()?;
                (text.starts_with("line")
                    && bytes[4..6].iter().all(u8::is_ascii_digit)
                    && &bytes[6..10] == b" = 1"
                    && bytes[10..12].iter().all(u8::is_ascii_digit))
                .then(|| text.to_owned())
            })
        };
        let mut frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("line01 = 101"), "{frame}");
        assert!(!frame.contains("line08 = 108"), "{frame}");
        for _ in 0..24 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            frame = rendered_review_frame(&mut terminal, &app);
            if frame.contains("line08 = 108") && !frame.contains("line01 = 101") {
                break;
            }
        }
        assert!(frame.contains("line08 = 108"), "{frame}");
        assert!(!frame.contains("line01 = 101"), "{frame}");
        let anchor = first_added(&frame).expect("visible added line");
        for _ in 0..2 {
            app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
            frame = rendered_review_frame(&mut terminal, &app);
            assert!(frame.contains(&anchor), "{frame}");
            assert_eq!(
                first_added(&frame).as_deref(),
                Some(anchor.as_str()),
                "{frame}"
            );
        }
    }

    #[test]
    fn layout_toggle_preserves_first_visible_source_line_after_arrow_scrolling() {
        let mut review = navigation_changeset(vec![(
            "scroll.ts".into(),
            numbered_exports(1, 18, 0, true),
            numbered_exports(1, 18, 100, true),
        )]);
        review.files[0].agent = Some(
            serde_json::from_value(serde_json::json!({
                "path":"scroll.ts", "summary":"scroll.ts note",
                "annotations":[{"new_range":{"start":2,"end":2}, "summary":"Annotation for scroll.ts",
                    "rationale":"Why scroll.ts changed"}]
            }))
            .unwrap(),
        );
        review.refresh_review_identities();
        assert_fixture_annotation_range(&review, 2);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
        let first_line = |frame: &str| {
            frame.as_bytes().windows(8).find_map(|bytes| {
                (bytes.starts_with(b"line")
                    && bytes[4..6].iter().all(u8::is_ascii_digit)
                    && &bytes[6..8] == b" =")
                    .then(|| String::from_utf8_lossy(&bytes[4..6]).into_owned())
            })
        };
        let mut frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("line01 = 101"), "{frame}");
        assert!(!frame.contains("line08 = 108"), "{frame}");
        for _ in 0..24 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            frame = rendered_review_frame(&mut terminal, &app);
            if frame.contains("line08 = 108") && !frame.contains("line01 = 101") {
                break;
            }
        }
        assert!(frame.contains("line08 = 108"), "{frame}");
        assert!(!frame.contains("line01 = 101"), "{frame}");
        let anchor = first_line(&frame).expect("visible source line");
        for (key, layout) in [('2', LayoutMode::Stack), ('1', LayoutMode::Split)] {
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            frame = rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.layout(), layout);
            assert!(frame.contains(&format!("line{anchor} =")), "{frame}");
            assert_eq!(
                first_line(&frame).as_deref(),
                Some(anchor.as_str()),
                "{frame}"
            );
        }
    }

    #[test]
    fn space_and_pageup_keep_code_visible_through_viewport_paging() {
        for path in ["space.ts", "pageup.ts"] {
            let review = navigation_changeset(vec![(
                path.into(),
                numbered_exports(1, 50, 0, true),
                numbered_exports(1, 50, 1000, true),
            )]);
            let mut app = ReviewApp::new(
                review,
                ReviewOptions {
                    layout: LayoutMode::Split,
                    ..Default::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
            let initial = rendered_review_frame(&mut terminal, &app);
            assert!(initial.contains("line01 = 1001"), "{initial}");
            app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
            let paged = rendered_review_frame(&mut terminal, &app);
            assert!(paged.contains("export const line"), "{paged}");
            assert!(app.scroll > 0);
            if path == "pageup.ts" {
                app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
                let restored = rendered_review_frame(&mut terminal, &app);
                assert!(restored.contains("export const line"), "{restored}");
                assert!(restored.contains("line01 = 1001"), "{restored}");
                assert_eq!(app.scroll, 0);
            }
        }
    }

    #[test]
    fn paging_aliases_and_shifted_g_preserve_source_key_sequences() {
        for (path, count) in [("half.ts", 50), ("g.ts", 120), ("pager-g.ts", 120)] {
            let review = navigation_changeset(vec![(
                path.into(),
                numbered_exports(1, count, 0, true),
                numbered_exports(1, count, 1000, true),
            )]);
            let mut app = ReviewApp::new(
                review,
                ReviewOptions {
                    pager: path == "pager-g.ts",
                    show_menu_bar: path != "pager-g.ts",
                    layout: LayoutMode::Split,
                    ..Default::default()
                },
            );
            let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
            let initial = rendered_review_frame(&mut terminal, &app);
            assert!(initial.contains("line01 = 1001"), "{initial}");
            if path == "half.ts" {
                for (key, modifiers) in [
                    ('d', KeyModifiers::NONE),
                    ('u', KeyModifiers::NONE),
                    ('f', KeyModifiers::NONE),
                    (' ', KeyModifiers::SHIFT),
                ] {
                    app.handle_key(KeyEvent::new(KeyCode::Char(key), modifiers));
                    let frame = rendered_review_frame(&mut terminal, &app);
                    assert!(frame.contains("export const line"), "{frame}");
                    assert!(!app.should_quit);
                }
            } else {
                app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::SHIFT));
                let bottom = rendered_review_frame(&mut terminal, &app);
                assert!(bottom.contains("line120 = 1120"), "{bottom}");
                app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
                let top = rendered_review_frame(&mut terminal, &app);
                assert!(top.contains("line01 = 1001"), "{top}");
            }
        }
    }

    #[test]
    fn tab_filter_input_renders_query_and_empty_match_message() {
        let mut review = responsive_changeset();
        review.files[0].agent = Some(
            serde_json::from_value(serde_json::json!({
                "path":"alpha.ts", "summary":"alpha.ts note",
                "annotations":[{"new_range":{"start":2,"end":2}, "summary":"Annotation for alpha.ts",
                    "rationale":"Why alpha.ts changed"}]
            }))
            .unwrap(),
        );
        review.refresh_review_identities();
        assert_fixture_annotation_range(&review, 2);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(240, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        for character in "zzz".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let frame = rendered_review_frame(&mut terminal, &app);
        for expected in ["filter:", "zzz", "No files match the current filter."] {
            assert!(frame.contains(expected), "missing {expected}:\n{frame}");
        }
        assert_eq!(app.focus, Focus::Filter);
        assert_eq!(app.filter, "zzz");
    }

    #[test]
    fn filter_displays_beta_but_preserves_hidden_selection_and_query_after_tab() {
        let mut review = responsive_changeset();
        review.files[0].agent = Some(
            serde_json::from_value(serde_json::json!({
                "path":"alpha.ts", "summary":"alpha.ts note",
                "annotations":[{"new_range":{"start":2,"end":2}, "summary":"Annotation for alpha.ts",
                    "rationale":"Why alpha.ts changed"}]
            }))
            .unwrap(),
        );
        review.refresh_review_identities();
        assert_fixture_annotation_range(&review, 2);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(240, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        assert_eq!(app.with_state(|state| state.selection().file_index), 0);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        for character in "beta".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let frame = rendered_review_frame(&mut terminal, &app);
        for expected in ["filter:", "beta", "beta.ts"] {
            assert!(frame.contains(expected), "missing {expected}:\n{frame}");
        }
        assert!(!frame.contains("Annotation for alpha.ts"), "{frame}");
        // Pinned core/selectors.ts retains a selected file still in the
        // document, even when hidden. The source interaction test's title is
        // older than that contract and asserts only the rendered filter state.
        assert_eq!(app.with_state(|state| state.selection().file_index), 0);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("filter=beta"), "{frame}");
        assert!(frame.contains("beta.ts"), "{frame}");
        assert_eq!(app.with_state(|state| state.selection().file_index), 0);
    }

    #[test]
    fn session_comment_navigation_keeps_active_filter_and_visible_files() {
        let mut review = responsive_changeset();
        review.files[0].agent = Some(
            serde_json::from_value(serde_json::json!({
                "path":"alpha.ts", "summary":"alpha.ts note",
                "annotations":[{"new_range":{"start":2,"end":2}, "summary":"Annotation for alpha.ts",
                    "rationale":"Why alpha.ts changed"}]
            }))
            .unwrap(),
        );
        review.refresh_review_identities();
        assert_fixture_annotation_range(&review, 2);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(240, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        for character in "beta".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let frame = rendered_review_frame(&mut terminal, &app);
        for expected in ["filter:", "beta", "betaValue"] {
            assert!(frame.contains(expected), "{frame}");
        }
        assert!(!frame.contains("add = true"), "{frame}");
        let selection = app.with_state(|state| state.selection());
        let input = serde_json::from_value(serde_json::json!({"commentDirection":"next"})).unwrap();
        let error = app.session_navigate_to_location(&input).unwrap_err();
        assert!(error.contains("No annotated hunks found in the current review."));
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("betaValue"), "{frame}");
        assert!(!frame.contains("add = true"), "{frame}");
        assert_eq!(app.filter, "beta");
        assert_eq!(app.with_state(|state| state.selection()), selection);
    }

    #[test]
    fn session_comment_navigation_reveals_deep_inline_note_and_returns_hunk() {
        let before = (1..=80)
            .map(|line| format!("export const line{line} = {line};"))
            .collect::<Vec<_>>();
        let mut after = before.clone();
        after[0] = "export const line1 = 100;".into();
        for line in 60..=65 {
            after[line - 1] = format!("export const line{line} = {};", line * 100);
        }
        let mut review = navigation_changeset(vec![(
            "deep-note.ts".into(),
            before.join("\n") + "\n",
            after.join("\n") + "\n",
        )]);
        review.files[0].agent = Some(
            serde_json::from_value(serde_json::json!({
                "path":"deep-note.ts", "summary":"file note",
                "annotations":[{"new_range":{"start":62,"end":62}, "summary":"Note anchored on second hunk."}]
            }))
            .unwrap(),
        );
        review.refresh_review_identities();
        assert_fixture_annotation_range(&review, 62);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                layout: LayoutMode::Split,
                agent_notes: true,
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(104, 18)).unwrap();
        let initial = rendered_review_frame(&mut terminal, &app);
        assert!(
            !initial.contains("Note anchored on second hunk."),
            "{initial}"
        );
        let input = serde_json::from_value(serde_json::json!({"commentDirection":"next"})).unwrap();
        let result = app.session_navigate_to_location(&input).unwrap();
        assert_eq!(result.file_path, "deep-note.ts");
        assert_eq!(result.hunk_index, 1);
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("Note anchored on second hunk."), "{frame}");
    }

    #[test]
    fn scroll_step_keys_in_pager_move_exactly_one_row() {
        let review = navigation_changeset(vec![(
            "scroll.ts".into(),
            numbered_exports(1, 60, 0, true),
            numbered_exports(1, 60, 100, true),
        )]);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                pager: true,
                cursor_line: CursorLineMode::Off,
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        for (key, expected) in [('j', 1), ('j', 2), ('k', 1)] {
            app.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
            rendered_review_frame(&mut terminal, &app);
            assert_eq!(app.scroll, expected);
        }
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
    fn review_scroll_activity_paints_and_then_hides_the_native_scrollbar() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            overflowing_changeset(12),
            ReviewOptions {
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert!(app.review_scrollbar_hits.get().is_none());

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 20,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let map = app
            .review_scrollbar_hits
            .get()
            .expect("overflowing review scrollbar");
        assert_eq!(
            terminal.backend().buffer()[map.thumb.as_position()].bg,
            ratatui_theme_color(&app.options.theme.accent_muted)
        );

        let deadline = app
            .review_scrollbar
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .hide_deadline()
            .expect("hide deadline");
        app.tick_extension_notifications(deadline - Duration::from_millis(1));
        assert!(
            app.review_scrollbar
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_visible()
        );
        app.tick_extension_notifications(deadline);
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert!(app.review_scrollbar_hits.get().is_none());
    }

    #[test]
    fn review_scrollbar_track_and_thumb_own_real_ratatui_pointer_geometry() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            overflowing_changeset(16),
            ReviewOptions {
                sidebar: false,
                ..ReviewOptions::default()
            },
        );
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        app.review_scrollbar
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .show(Instant::now());
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let map = app.review_scrollbar_hits.get().expect("scrollbar map");

        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: map.track.x,
            row: map.track.bottom().saturating_sub(1),
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll, map.geometry.track_height);

        app.scroll = 0;
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let map = app.review_scrollbar_hits.get().expect("scrollbar map");
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: map.thumb.x,
            row: map.thumb.y,
            modifiers: KeyModifiers::NONE,
        });
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[map.thumb.as_position()].bg,
            ratatui_theme_color(&app.options.theme.accent)
        );

        let drag_rows = 4_u16.min(map.track.height.saturating_sub(1));
        let expected = (f64::from(drag_rows)
            / (map.geometry.max_thumb_y as f64 / map.geometry.max_scroll as f64))
            .clamp(0.0, map.geometry.max_scroll as f64)
            .round() as usize;
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: map.thumb.x,
            row: map.thumb.y.saturating_add(drag_rows),
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.scroll, expected);
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: map.thumb.x,
            row: map.thumb.y.saturating_add(drag_rows),
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.review_scrollbar
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .hide_deadline()
                .is_some()
        );
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
    fn theme_selector_keyboard_previews_reverts_accepts_and_reopens_on_commit() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let committed = app.themes.committed.clone();

        app.apply_builtin_command_action(AppCommandAction::OpenThemeSelector);
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
        assert!(rendered.contains("Theme selector"));
        assert!(rendered.contains("Enter/click accept  Esc cancel"));
        assert!(app.themes.selector_open);

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let preview = app.options.theme.id.clone();
        assert_ne!(preview, committed);
        assert_eq!(app.themes.committed, committed);
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(!app.should_quit);
        assert_eq!(app.options.theme.id, preview);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.themes.selector_open);
        assert_eq!(app.options.theme.id, committed);

        app.apply_builtin_command_action(AppCommandAction::OpenThemeSelector);
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let accepted = app.options.theme.id.clone();
        let accepted_label = app
            .theme_catalog()
            .into_iter()
            .find(|theme| theme.id == accepted)
            .unwrap()
            .label;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.themes.selector_open);
        assert_eq!(app.themes.committed, accepted);
        let expected_status = format!("Theme: {accepted_label}");
        assert_eq!(app.status.as_deref(), Some(expected_status.as_str()));

        app.apply_builtin_command_action(AppCommandAction::OpenThemeSelector);
        let catalog = app.theme_catalog();
        let committed_index = catalog
            .iter()
            .position(|theme| theme.id == app.themes.committed)
            .unwrap();
        assert_eq!(app.themes.selected_index(&catalog), committed_index);
    }

    #[test]
    fn configured_vertical_review_keys_move_the_open_theme_selector() {
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                keybindings: vec![UserKeyBindingEntry::new(
                    "workdeck.review.nextHunk",
                    UserKeyBinding::Chord("x".into()),
                )],
                ..ReviewOptions::default()
            },
        );
        app.apply_builtin_command_action(AppCommandAction::OpenThemeSelector);
        let before = app.themes.selected_index(&app.theme_catalog());
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            app.themes.selected_index(&app.theme_catalog()),
            (before + 1) % app.theme_catalog().len()
        );
    }

    #[test]
    fn custom_theme_catalog_and_palette_remain_active_in_the_selector() {
        let custom: NamedCustomThemeConfig = serde_json::from_value(serde_json::json!({
            "id": "team-dark",
            "label": "Team Dark",
            "base": "github-dark-default",
            "accent": "#8877cc"
        }))
        .unwrap();
        let theme = resolve_theme(Some(&custom.id), None, std::slice::from_ref(&custom));
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                theme,
                custom_themes: vec![custom],
                ..ReviewOptions::default()
            },
        );
        app.apply_builtin_command_action(AppCommandAction::OpenThemeSelector);
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let catalog = app.theme_catalog();
        let index = catalog
            .iter()
            .position(|theme| theme.id == "team-dark")
            .unwrap();
        assert_eq!(app.themes.selected_index(&catalog), index);
        assert_eq!(app.options.theme.accent, "#8877cc");
        assert!(app.themes.items(&catalog)[index].active);
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("Team Dark")
        );
    }

    #[test]
    fn custom_theme_stays_active_when_opened_through_view_menu() {
        let custom: NamedCustomThemeConfig = serde_json::from_value(serde_json::json!({
            "id": "custom", "base": "github-light-default",
            "label": "My Theme", "accent": "#7755aa"
        }))
        .unwrap();
        let theme = resolve_theme(Some("custom"), None, std::slice::from_ref(&custom));
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                theme,
                custom_themes: vec![custom],
                ..Default::default()
            },
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 20)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert!(rendered_review_frame(&mut terminal, &app).contains("Toggle files/filter focus"));
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        rendered_review_frame(&mut terminal, &app);
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        let frame = rendered_review_frame(&mut terminal, &app);
        assert!(frame.contains("Theme selector"), "{frame}");
        assert!(frame.contains("›  My Theme"), "{frame}");
        assert!(frame.contains("active"), "{frame}");
        assert_eq!(app.options.theme.id, "custom");
        assert_eq!(app.options.theme.accent, "#7755aa");
    }

    #[test]
    fn theme_selector_pointer_dwell_wheel_click_and_backdrop_follow_source_timing() {
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let committed = app.themes.committed.clone();
        app.apply_builtin_command_action(AppCommandAction::OpenThemeSelector);
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let initial_plan = app
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .unwrap();
        let target = initial_plan
            .item_hits
            .iter()
            .find(|hit| hit.id != committed)
            .unwrap()
            .clone();
        let start = Instant::now();
        app.handle_theme_selector_mouse(
            &MouseEvent {
                kind: MouseEventKind::Moved,
                column: target.bounds.x,
                row: target.bounds.y,
                modifiers: KeyModifiers::NONE,
            },
            start,
        );
        app.tick_theme_hover_preview(start + Duration::from_millis(199));
        assert_eq!(app.options.theme.id, committed);
        app.tick_theme_hover_preview(start + Duration::from_millis(200));
        assert_eq!(app.options.theme.id, target.id);
        assert_eq!(app.themes.committed, committed);

        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let before_scroll = app
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .unwrap()
            .window
            .window_start;
        app.handle_theme_selector_mouse(
            &MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: target.bounds.x,
                row: target.bounds.y,
                modifiers: KeyModifiers::NONE,
            },
            start + Duration::from_millis(201),
        );
        assert_eq!(app.options.theme.id, target.id);
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let scrolled_plan = app
            .theme_selector_dialog_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .unwrap();
        assert_eq!(
            scrolled_plan.window.window_start,
            (before_scroll + 1).min(
                scrolled_plan
                    .window
                    .item_count
                    .saturating_sub(scrolled_plan.window.visible_rows)
            )
        );

        let accepted = scrolled_plan
            .item_hits
            .iter()
            .find(|hit| hit.id != target.id)
            .unwrap()
            .clone();
        app.handle_theme_selector_mouse(
            &MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: accepted.bounds.x,
                row: accepted.bounds.y,
                modifiers: KeyModifiers::NONE,
            },
            start + Duration::from_millis(202),
        );
        assert!(!app.themes.selector_open);
        assert_eq!(app.themes.committed, accepted.id);

        app.apply_builtin_command_action(AppCommandAction::OpenThemeSelector);
        app.move_theme_selector(1);
        assert_ne!(app.options.theme.id, app.themes.committed);
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        app.handle_theme_selector_mouse(
            &MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            start + Duration::from_millis(203),
        );
        assert!(!app.themes.selector_open);
        assert_eq!(app.options.theme.id, app.themes.committed);
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
    fn repeated_escape_clears_each_retyped_no_match_filter_before_exiting() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/filter-escape.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["source"],
            serde_json::json!({
                "commit": "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                "path": "test/pty/filter-escape.test.ts",
                "blob": "b386d05a6575ae0f0ee1e31b10034bb5fe02b369",
                "sha256": "f8d8d8bba4f1b0ac34471a69e2ae9563da9b4a83858b652e56bc123de4338a5a",
                "bytes": 2_054,
                "lines": 63,
                "stable_v0_20_1": "absent"
            })
        );
        assert_eq!(oracle["execution"]["runtime"], "Bun 1.3.14");
        assert_eq!(oracle["execution"]["passed"], 1);
        assert_eq!(oracle["execution"]["failed"], 0);
        assert_eq!(oracle["execution"]["expect_calls"], 3);
        assert_eq!(oracle["terminal"]["columns"], 220);
        assert_eq!(oracle["terminal"]["rows"], 12);
        assert_eq!(oracle["source_tests"].as_array().unwrap().len(), 1);
        assert_eq!(
            oracle["native_test"]["test"],
            "tests::repeated_escape_clears_each_retyped_no_match_filter_before_exiting"
        );

        let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
        let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
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
        let first_clear = rendered_review_frame(&mut terminal, &app);
        assert!(first_clear.contains("filter: type to filter files"));
        assert!(first_clear.contains("a.rs"));
        assert!(!first_clear.contains("No files match"));

        for character in "zzz".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let no_match = rendered_review_frame(&mut terminal, &app);
        assert_eq!(app.filter, "zzz");
        assert_eq!(app.focus, Focus::Filter);
        assert!(no_match.contains("No files match the current filter."));

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.filter.is_empty());
        assert_eq!(app.focus, Focus::Filter);
        let second_clear = rendered_review_frame(&mut terminal, &app);
        assert!(second_clear.contains("filter: type to filter files"));
        assert!(second_clear.contains("a.rs"));
        assert!(!second_clear.contains("No files match"));

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

    #[test]
    fn dirty_view_quit_renders_saves_and_exits_only_after_the_notice_window() {
        let directory = tempfile::TempDir::new().unwrap();
        let config_path = directory.path().join(".config/workdeck/config.toml");
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                view_preferences_config_path: Some(config_path.clone()),
                view_preferences_home_directory: Some(directory.path().to_owned()),
                ..ReviewOptions::default()
            },
        );
        app.apply_builtin_command_action(AppCommandAction::ToggleLineWrap);
        app.show_help = true;
        app.apply_builtin_command_action(AppCommandAction::RequestQuit);
        assert!(app.save_config_prompt_open());
        assert!(!app.show_help);
        assert!(!app.take_quit_requested());

        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let frame = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(frame.contains("Save view preferences?"));
        assert!(frame.contains("~/.config/workdeck/config.toml"));
        assert!(frame.contains("- wrap_lines = false"));
        assert!(frame.contains("+ wrap_lines = true"));
        assert!(frame.contains("enter/s save"));
        assert!(frame.contains("q discard"));
        assert!(frame.contains("n never ask"));

        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(app.save_config_prompt_open());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.save_config_prompt_open());
        assert_eq!(app.changed_view_preferences().len(), 1);

        app.request_quit();
        let start = Instant::now();
        app.save_view_preferences_and_quit(start);
        assert!(!app.save_config_prompt_open());
        assert!(
            std::fs::read_to_string(config_path)
                .unwrap()
                .contains("wrap_lines = true")
        );
        assert!(app.changed_view_preferences().is_empty());
        app.tick_extension_notifications(start + Duration::from_millis(119));
        assert!(!app.take_quit_requested());
        app.tick_extension_notifications(start + POST_PERSISTENCE_QUIT_DELAY);
        assert!(app.take_quit_requested());
    }

    #[test]
    fn dirty_view_quit_prompt_mouse_actions_are_modal_and_clickable() {
        let directory = tempfile::TempDir::new().unwrap();
        let config_path = directory.path().join("config.toml");
        let mut app = ReviewApp::new(
            changeset(),
            ReviewOptions {
                view_preferences_config_path: Some(config_path.clone()),
                ..ReviewOptions::default()
            },
        );
        app.apply_builtin_command_action(AppCommandAction::ToggleLineWrap);
        app.request_quit();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 100, 24));
        render(buffer.area, &mut buffer, &app);
        let discard = app
            .view_preference_prompt_hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .unwrap()
            .action_hits[1]
            .bounds;
        app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: discard.x,
            row: discard.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(!app.save_config_prompt_open());
        assert!(app.take_quit_requested());
        assert!(!config_path.exists());
    }
}
