mod extension_cli_commands;
mod extension_manage;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use workdeck_cli::app::App;
use workdeck_cli::config::Config;
use workdeck_cli::git;
use workdeck_cli::payload::{
    changes_grouped_by_status, file_preview_payload, search_target_group, search_target_payload,
    status_payload,
};
use workdeck_cli::store::{
    AgentSession, AgentTouchedFile, Cycle, Issue, IssueStatus, IssueUpdate, Label, Priority,
    Project, ReferenceData, StoreEvent, WorkdeckStore,
};
use workdeck_core::{
    AgentContext, AppBootstrap, Changeset, ChangesetSource, CliInput, CommonOptions,
    DiffToolCommandInput, FileCommandInput, InputCursorLine, InputLayoutMode,
    NamedCustomThemeConfig, PatchCommandInput, RegisteredCustomTheme, ReloadContext, ReviewSide,
    SelfUpdateCommandInput, SidebarVisibility, StartupNotice, TerminalThemeMode,
    UserKeyBindingEntry, VcsDiffCommandInput, VcsRangeEndpoints, VcsShowCommandInput,
    VcsStashShowCommandInput, collect_session_custom_themes, resolve_app_state_path,
};
use workdeck_diff::{
    LanguageMatcher, LanguageRegistration, LanguageRegistry, sanitize_terminal_line,
};
use workdeck_extension_api::{
    CliCommandResult, ExtensionManifest, ExtensionNotificationHub, ExtensionNotifyType,
    FileLanguageGlobTarget, FileLanguageMatcher, Registration,
};
use workdeck_extension_host::{
    ExtensionLoadResult, LoadStartupExtensionsOptions, LoadedExtension, TrustDecision, TrustStore,
    create_empty_extension_load_result, create_extension_apply_notices,
    create_extension_load_notices, discover_manifests_with_config, load_startup_extensions,
    resolve_loaded_extension_registrations, resolved_native_vcs_adapters,
};
use workdeck_review::{
    CommentTargetInput, LayoutMode, ReviewComment, build_live_comment, find_diff_file_by_path,
    resolve_comment_target,
};
use workdeck_session::{
    SelectableSession, SessionAction, SessionClient, SessionDescriptor, SessionSelector,
    decode_snapshot, default_discovery_directory, normalize_session_selector,
    repo_selector_distance,
};
use workdeck_tui::{
    CursorLineMode, ExtensionTrustHandler, ExtensionTrustHostError, ExtensionTrustWriteError,
    ReviewOptions, ThemeProbeInput,
};
use workdeck_vcs::{
    AnyProvider, ProviderPreference, VcsAdapter, VcsCatalog, VcsLoadContext, VcsReviewInput,
    WatchSignatureContext, bundled_vcs_catalog, compute_watch_signature, detect_vcs,
    extend_vcs_catalog, find_project_root_candidate, find_project_root_candidate_with_catalog,
    get_default_vcs_adapter, get_vcs_adapter, load_difftool_comparison, load_file_comparison,
    load_vcs_review, materialize_vcs_patch_result, operation_from_input, parse_patch_input,
};

use crate::extension_cli_commands::{
    ExtensionCliInterruptAction, ExtensionCliSignalLease, RegisteredExtensionCliCommand,
    create_extension_cli_collision_issues, describe_extension_cli_commands,
    find_extension_cli_command, resolve_extension_cli_commands,
};
use crate::extension_manage::{ExtensionManager, parse_extension_install_source};

#[derive(Debug, Parser)]
#[command(name = "workdeck")]
#[command(about = "Terminal-native sidecar for agentic coding")]
#[command(version)]
struct Args {
    #[arg(long, value_name = "PATH", default_value = ".")]
    cwd: PathBuf,

    #[arg(long, help = "Initialize .agents/workdeck without opening the TUI")]
    init: bool,

    #[arg(long, help = "Print a JSON status snapshot without opening the TUI")]
    status_json: bool,

    #[arg(long, global = true, value_name = "PATH")]
    extension: Vec<PathBuf>,

    #[arg(long, global = true, conflicts_with = "no_extensions")]
    extensions: bool,

    #[arg(long = "no-extensions", global = true, conflicts_with = "extensions")]
    no_extensions: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Review working tree changes or compare revisions")]
    Diff {
        #[arg(value_name = "REVISION", num_args = 0..=2)]
        revisions: Vec<String>,
        #[arg(long, value_name = "PATH", num_args = 1..)]
        files: Vec<PathBuf>,
        #[arg(long, alias = "cached", help = "Review staged changes")]
        staged: bool,
        #[arg(long, help = "Hide untracked files")]
        exclude_untracked: bool,
        #[arg(
            long,
            conflicts_with = "exclude_untracked",
            help = "Include untracked files"
        )]
        include_untracked: bool,
        #[arg(last = true, value_name = "PATHSPEC")]
        pathspec: Vec<String>,
        #[command(flatten)]
        review: ReviewCliOptions,
    },
    #[command(about = "Review the last commit or a given revision")]
    Show {
        target: Option<String>,
        #[arg(last = true, value_name = "PATHSPEC")]
        pathspec: Vec<String>,
        #[command(flatten)]
        review: ReviewCliOptions,
    },
    #[command(about = "Review Git stash entries")]
    Stash {
        #[command(subcommand)]
        command: StashCommand,
    },
    #[command(about = "Review a patch file or standard input")]
    Patch {
        file: Option<PathBuf>,
        #[command(flatten)]
        review: ReviewCliOptions,
    },
    #[command(about = "Review a concrete pair of files")]
    Difftool {
        left: PathBuf,
        right: PathBuf,
        path: Option<PathBuf>,
        #[command(flatten)]
        review: ReviewCliOptions,
    },
    #[command(about = "Read Git pager input and open review mode when it contains a patch")]
    Pager {
        #[command(flatten)]
        review: ReviewCliOptions,
    },
    #[command(about = "Inspect and control live Workdeck review sessions")]
    Session {
        #[command(subcommand)]
        command: LiveSessionCommand,
    },
    #[command(about = "Manage native Workdeck extensions")]
    Extension {
        #[command(subcommand)]
        command: ExtensionCommand,
    },
    #[command(about = "Migrate data from another review tool")]
    Migrate {
        #[command(subcommand)]
        command: MigrateCommand,
    },
    #[command(about = "Render or inspect terminal-safe Workdeck markup")]
    Markup {
        #[command(subcommand)]
        command: MarkupCommand,
    },
    #[command(about = "Materialize bundled Workdeck agent skills")]
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    #[command(about = "Update Workdeck through the channel that installed it")]
    Update {
        #[arg(value_name = "VERSION", help = "Install an exact release version")]
        version: Option<String>,
        #[arg(
            long,
            value_name = "METHOD",
            help = "Override detection: cargo, brew, nix, curl, powershell, or direct"
        )]
        method: Option<String>,
        #[arg(long, help = "Report versions without installing")]
        check: bool,
    },
    #[command(about = "Print a Git status snapshot")]
    Status {
        #[arg(long, help = "Print status as JSON")]
        json: bool,
    },
    #[command(about = "Inspect repo files without opening the TUI")]
    Files {
        #[command(subcommand)]
        command: FilesCommand,
    },
    #[command(about = "Inspect Git changes without opening the TUI")]
    Changes {
        #[command(subcommand)]
        command: ChangesCommand,
    },
    #[command(about = "Search files, changes, issues, and agent data")]
    Search {
        query: String,
        #[arg(
            long,
            value_delimiter = ',',
            help = "Limit targets: files,changes,issues,agents"
        )]
        target: Vec<String>,
        #[arg(long, help = "Print search results as JSON")]
        json: bool,
    },
    #[command(about = "Manage Workdeck config")]
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    #[command(about = "Inspect Workdeck event log")]
    Events {
        #[command(subcommand)]
        command: EventsCommand,
    },
    #[command(about = "Import Workdeck export JSON")]
    Import {
        path: PathBuf,
        #[arg(long, conflicts_with = "replace", help = "Merge imported data")]
        merge: bool,
        #[arg(
            long,
            conflicts_with = "merge",
            help = "Replace local Workdeck data before import"
        )]
        replace: bool,
        #[arg(long, help = "Validate without writing")]
        dry_run: bool,
        #[arg(long, help = "Print import result as JSON")]
        json: bool,
    },
    #[command(about = "Validate repo, config, and local Workdeck data")]
    Doctor {
        #[arg(long, help = "Print doctor results as JSON")]
        json: bool,
    },
    #[command(about = "Export local Workdeck data as JSON or JSONL")]
    Export {
        #[arg(long, help = "Emit JSON; default unless --jsonl is used")]
        json: bool,
        #[arg(long, help = "Emit one JSON object per line")]
        jsonl: bool,
    },
    #[command(about = "Manage local Workdeck issues")]
    Issue {
        #[command(subcommand)]
        command: IssueCommand,
    },
    #[command(about = "Manage local agent sessions")]
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    #[command(about = "Manage local Workdeck projects")]
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    #[command(about = "Manage local Workdeck cycles")]
    Cycle {
        #[command(subcommand)]
        command: CycleCommand,
    },
    #[command(about = "Manage local Workdeck labels")]
    Label {
        #[command(subcommand)]
        command: LabelCommand,
    },
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum LiveSessionCommand {
    #[command(about = "List live Workdeck review sessions")]
    List {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Show one live Workdeck review session")]
    Get {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Show the active review selection")]
    Context {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Export the live provider-neutral review model")]
    Review {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        include_patch: bool,
        #[arg(long)]
        include_source: bool,
        #[arg(long)]
        include_notes: bool,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Reload one live review from its original bounded input")]
    Reload {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Move a live review to one file, hunk, or line")]
    Navigate {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, conflicts_with_all = ["old_line", "new_line"])]
        hunk: Option<usize>,
        #[arg(long, conflicts_with_all = ["hunk", "new_line"])]
        old_line: Option<u32>,
        #[arg(long, conflicts_with_all = ["hunk", "old_line"])]
        new_line: Option<u32>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Manage live inline review comments")]
    Comment {
        #[command(subcommand)]
        command: LiveCommentCommand,
    },
    #[command(about = "Ask a live review to quit")]
    Quit {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum LiveCommentCommand {
    #[command(about = "Attach one live inline review note")]
    Add {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, conflicts_with_all = ["new_line", "hunk_index"])]
        old_line: Option<u32>,
        #[arg(long, conflicts_with_all = ["old_line", "hunk_index"])]
        new_line: Option<u32>,
        #[arg(long, conflicts_with_all = ["old_line", "new_line"])]
        hunk_index: Option<usize>,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        rationale: Option<String>,
        #[arg(long, value_name = "STML")]
        markup: Option<String>,
        #[arg(long)]
        author: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "List live inline review notes")]
    List {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Remove one live inline review note")]
    Remove {
        id: Option<String>,
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(value_name = "COMMENT_ID")]
        comment_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ExtensionCommand {
    #[command(about = "Install a shared native extension from a Git repository")]
    Install {
        source: String,
        #[arg(long, help = "Skip the native-code trust confirmation")]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "List discovered and managed native extensions")]
    List {
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Re-clone managed native extensions from their recorded sources")]
    Update {
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Remove one managed native extension")]
    Remove {
        name: String,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Validate one native extension manifest")]
    Validate {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Grant or deny repository-native extension trust")]
    Trust {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, conflicts_with = "deny")]
        allow: bool,
        #[arg(long, conflicts_with = "allow")]
        deny: bool,
        #[arg(long, help = "Confirm the trust decision")]
        yes: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum MigrateCommand {
    #[command(about = "Plan or apply one-time Hunk configuration migration")]
    Hunk {
        #[arg(long, conflicts_with = "apply")]
        dry_run: bool,
        #[arg(long, conflicts_with = "dry_run")]
        apply: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum MarkupColor {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Subcommand)]
enum MarkupCommand {
    #[command(about = "Render STML from a file or standard input")]
    Render {
        #[arg(default_value = "-")]
        file: PathBuf,
        #[arg(long, default_value_t = workdeck_markup::DEFAULT_WIDTH)]
        width: usize,
        #[arg(long, value_enum, default_value = "auto")]
        color: MarkupColor,
        #[arg(long)]
        theme: Option<String>,
        #[arg(long)]
        json: bool,
    },
    #[command(about = "Print the STML authoring guide")]
    Guide,
}

#[derive(Debug, Subcommand)]
enum SkillCommand {
    #[command(about = "Install a bundled skill locally and print its path")]
    Path {
        #[arg(default_value = "workdeck-review")]
        name: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum StashCommand {
    #[command(about = "Review a stash entry")]
    Show {
        reference: Option<String>,
        #[command(flatten)]
        review: ReviewCliOptions,
    },
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum ReviewLayoutArg {
    #[default]
    Auto,
    Split,
    Stack,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum CursorLineArg {
    #[default]
    Row,
    Number,
    Off,
}

#[derive(Debug, Clone, Default, ClapArgs)]
struct ReviewCliOptions {
    #[arg(long, value_name = "ID")]
    vcs: Option<String>,
    #[arg(long, value_enum)]
    mode: Option<ReviewLayoutArg>,
    #[arg(
        long,
        conflicts_with = "no_watch",
        help = "Reload when the review input changes"
    )]
    watch: bool,
    #[arg(long = "no-watch", conflicts_with = "watch")]
    no_watch: bool,
    #[arg(long, help = "Use pager-style chrome")]
    pager: bool,
    #[arg(long, conflicts_with = "no_line_numbers")]
    line_numbers: bool,
    #[arg(long = "no-line-numbers")]
    no_line_numbers: bool,
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=16))]
    tab_width: Option<u16>,
    #[arg(long, value_enum)]
    cursor_line: Option<CursorLineArg>,
    #[arg(long, conflicts_with = "no_wrap")]
    wrap: bool,
    #[arg(long = "no-wrap")]
    no_wrap: bool,
    #[arg(long, conflicts_with = "no_hunk_headers")]
    hunk_headers: bool,
    #[arg(long = "no-hunk-headers")]
    no_hunk_headers: bool,
    #[arg(long, conflicts_with = "no_sidebar")]
    sidebar: bool,
    #[arg(long = "no-sidebar")]
    no_sidebar: bool,
    #[arg(skip)]
    sidebar_visibility: SidebarVisibility,
    #[arg(long, conflicts_with = "no_agent_notes")]
    agent_notes: bool,
    #[arg(long = "no-agent-notes")]
    no_agent_notes: bool,
    #[arg(long, value_parser = clap::value_parser!(u16).range(0..=8))]
    file_gap: Option<u16>,
    #[arg(long, value_parser = clap::value_parser!(u16).range(0..=8))]
    hunk_gap: Option<u16>,
    #[arg(long = "transparent-bg", conflicts_with = "opaque_background")]
    transparent_background: bool,
    #[arg(long = "opaque-bg", conflicts_with = "transparent_background")]
    opaque_background: bool,
    #[arg(long, value_name = "PATH")]
    agent_context: Option<PathBuf>,
    #[arg(long, value_name = "THEME")]
    theme: Option<String>,
    #[arg(skip)]
    initial_theme_mode: Option<TerminalThemeMode>,
    #[arg(skip)]
    extension: Vec<PathBuf>,
    #[arg(skip)]
    no_extensions: bool,
    #[arg(skip)]
    color_moved: Option<bool>,
    #[arg(skip)]
    show_menu_bar: bool,
    #[arg(skip)]
    copy_decorations: bool,
    #[arg(skip)]
    keybindings: Vec<UserKeyBindingEntry>,
    #[arg(skip)]
    keybinding_notices: Vec<String>,
    #[arg(skip)]
    startup_notices: Vec<StartupNotice>,
    #[arg(skip)]
    extension_config: BTreeMap<String, Value>,
    #[arg(skip)]
    user_extension_paths: Vec<PathBuf>,
    #[arg(skip)]
    repo_extension_paths: Vec<PathBuf>,
    #[arg(skip)]
    custom_themes: Vec<NamedCustomThemeConfig>,
    #[arg(skip)]
    view_preferences_config_path: Option<PathBuf>,
    #[arg(skip)]
    config_exclude_untracked: bool,
}

impl ReviewCliOptions {
    fn from_config(config: &Config) -> Self {
        Self {
            vcs: Some(config.review.vcs.clone()),
            mode: Some(match config.review.mode.as_str() {
                "split" => ReviewLayoutArg::Split,
                "stack" => ReviewLayoutArg::Stack,
                _ => ReviewLayoutArg::Auto,
            }),
            watch: config.review.watch,
            no_watch: !config.review.watch,
            pager: false,
            line_numbers: config.review.line_numbers,
            no_line_numbers: !config.review.line_numbers,
            tab_width: Some(config.review.tab_width),
            cursor_line: Some(match config.review.cursor_line.as_str() {
                "number" => CursorLineArg::Number,
                "off" => CursorLineArg::Off,
                _ => CursorLineArg::Row,
            }),
            wrap: config.review.wrap_lines,
            no_wrap: !config.review.wrap_lines,
            hunk_headers: config.review.hunk_headers,
            no_hunk_headers: !config.review.hunk_headers,
            sidebar: false,
            no_sidebar: false,
            sidebar_visibility: match config.review.sidebar {
                workdeck_cli::config::ReviewSidebar::Auto => SidebarVisibility::Auto,
                workdeck_cli::config::ReviewSidebar::Show => SidebarVisibility::Visible,
                workdeck_cli::config::ReviewSidebar::Hide => SidebarVisibility::Hidden,
            },
            agent_notes: config.review.agent_notes,
            no_agent_notes: !config.review.agent_notes,
            file_gap: Some(config.review.file_gap),
            hunk_gap: Some(config.review.hunk_gap),
            transparent_background: config.review.transparent_background,
            opaque_background: !config.review.transparent_background,
            agent_context: None,
            theme: (config.ui.theme != "auto").then(|| config.ui.theme.clone()),
            initial_theme_mode: None,
            extension: Vec::new(),
            no_extensions: !config.resolved_extensions.enabled,
            color_moved: config.review.color_moved,
            show_menu_bar: config.review.menu_bar,
            copy_decorations: config.review.copy_decorations,
            keybindings: config.keybindings.clone(),
            keybinding_notices: config.keybinding_notices.clone(),
            startup_notices: config.startup_notices.clone(),
            extension_config: config.extension_configs().clone(),
            user_extension_paths: config.resolved_extensions.paths.clone(),
            repo_extension_paths: config.resolved_extensions.repo_paths.clone(),
            custom_themes: config.custom_themes.clone(),
            view_preferences_config_path: config.view_preferences_config_path.clone(),
            config_exclude_untracked: config.review.exclude_untracked,
        }
    }

    fn apply_config_defaults(&mut self, config: &Config) {
        let configured = Self::from_config(config);
        if self.vcs.is_none() {
            self.vcs = configured.vcs;
        }
        self.mode = self.mode.or(configured.mode);
        self.tab_width = self.tab_width.or(configured.tab_width);
        self.cursor_line = self.cursor_line.or(configured.cursor_line);
        self.file_gap = self.file_gap.or(configured.file_gap);
        self.hunk_gap = self.hunk_gap.or(configured.hunk_gap);
        if !self.watch && !self.no_watch {
            self.watch = configured.watch;
            self.no_watch = configured.no_watch;
        }
        if !self.line_numbers && !self.no_line_numbers {
            self.line_numbers = configured.line_numbers;
            self.no_line_numbers = configured.no_line_numbers;
        }
        if !self.wrap && !self.no_wrap {
            self.wrap = configured.wrap;
            self.no_wrap = configured.no_wrap;
        }
        if !self.hunk_headers && !self.no_hunk_headers {
            self.hunk_headers = configured.hunk_headers;
            self.no_hunk_headers = configured.no_hunk_headers;
        }
        if !self.sidebar && !self.no_sidebar {
            self.sidebar_visibility = configured.sidebar_visibility;
        }
        if !self.agent_notes && !self.no_agent_notes {
            self.agent_notes = configured.agent_notes;
            self.no_agent_notes = configured.no_agent_notes;
        }
        if !self.transparent_background && !self.opaque_background {
            self.transparent_background = configured.transparent_background;
            self.opaque_background = configured.opaque_background;
        }
        if self.theme.is_none() {
            self.theme = configured.theme;
        }
        self.color_moved = self.color_moved.or(configured.color_moved);
        self.show_menu_bar = configured.show_menu_bar;
        self.copy_decorations = configured.copy_decorations;
        self.keybindings = configured.keybindings;
        self.keybinding_notices = configured.keybinding_notices;
        self.startup_notices = configured.startup_notices;
        self.extension_config = configured.extension_config;
        self.user_extension_paths = configured.user_extension_paths;
        self.repo_extension_paths = configured.repo_extension_paths;
        self.custom_themes = configured.custom_themes;
        self.view_preferences_config_path = configured.view_preferences_config_path;
        self.config_exclude_untracked = configured.config_exclude_untracked;
        self.no_extensions |= configured.no_extensions;
    }

    fn preference(&self) -> ProviderPreference {
        match self.vcs.as_deref() {
            Some("git") => ProviderPreference::Git,
            Some("jj") => ProviderPreference::Jujutsu,
            Some("sl") => ProviderPreference::Sapling,
            _ => ProviderPreference::Auto,
        }
    }

    fn configured_vcs_id(&self) -> Option<&str> {
        self.vcs.as_deref().filter(|id| *id != "auto")
    }

    fn tui_options(&self) -> ReviewOptions {
        let theme = workdeck_tui::resolve_theme(self.theme.as_deref(), None, &[]);
        let theme = if self.transparent_background && !self.opaque_background {
            workdeck_tui::with_transparent_surfaces(&theme)
        } else {
            theme
        };
        ReviewOptions {
            layout: match self.mode.unwrap_or(ReviewLayoutArg::Auto) {
                ReviewLayoutArg::Auto => LayoutMode::Auto,
                ReviewLayoutArg::Split => LayoutMode::Split,
                ReviewLayoutArg::Stack => LayoutMode::Stack,
            },
            sidebar: self.resolved_sidebar() != SidebarVisibility::Hidden,
            line_numbers: self.line_numbers || !self.no_line_numbers,
            tab_width: self.tab_width.unwrap_or(4),
            cursor_line: match self.cursor_line.unwrap_or(CursorLineArg::Row) {
                CursorLineArg::Row => CursorLineMode::Row,
                CursorLineArg::Number => CursorLineMode::Number,
                CursorLineArg::Off => CursorLineMode::Off,
            },
            hunk_headers: self.hunk_headers || !self.no_hunk_headers,
            wrap_lines: if self.no_wrap { false } else { self.wrap },
            horizontal_offset: 0,
            line_number_digits: None,
            highlight: true,
            file_gap: self.file_gap.unwrap_or(1),
            hunk_gap: self.hunk_gap.unwrap_or(0),
            transparent_background: self.transparent_background && !self.opaque_background,
            pager: self.pager,
            watch: self.watch && !self.no_watch,
            agent_notes: self.agent_notes && !self.no_agent_notes,
            show_menu_bar: self.show_menu_bar,
            copy_decorations: self.copy_decorations,
            theme,
            repo: None,
            command_cwd: None,
            review_input: None,
            keybindings: self.keybindings.clone(),
            keybinding_notices: self.keybinding_notices.clone(),
            startup_notices: Vec::new(),
            extension_panes: Vec::new(),
            extension_notifications: None,
            pending_extension_trust_repo_root: None,
            extension_trust_handler: None,
        }
    }

    fn common_options(&self) -> CommonOptions {
        CommonOptions {
            mode: Some(match self.mode.unwrap_or(ReviewLayoutArg::Auto) {
                ReviewLayoutArg::Auto => InputLayoutMode::Auto,
                ReviewLayoutArg::Split => InputLayoutMode::Split,
                ReviewLayoutArg::Stack => InputLayoutMode::Stack,
            }),
            cursor_line: Some(match self.cursor_line.unwrap_or(CursorLineArg::Row) {
                CursorLineArg::Row => InputCursorLine::Row,
                CursorLineArg::Number => InputCursorLine::Number,
                CursorLineArg::Off => InputCursorLine::Off,
            }),
            vcs: self
                .vcs
                .as_deref()
                .filter(|id| *id != "auto")
                .map(str::to_owned),
            theme: self.theme.clone(),
            agent_context: self
                .agent_context
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            pager: Some(self.pager),
            watch: Some(self.watch && !self.no_watch),
            exclude_untracked: None,
            line_numbers: Some(self.line_numbers || !self.no_line_numbers),
            tab_width: Some(self.tab_width.unwrap_or(4)),
            file_gap: Some(self.file_gap.unwrap_or(1)),
            hunk_gap: Some(self.hunk_gap.unwrap_or(0)),
            wrap_lines: Some(if self.no_wrap { false } else { self.wrap }),
            hunk_headers: Some(self.hunk_headers || !self.no_hunk_headers),
            sidebar: Some(self.resolved_sidebar()),
            agent_notes: Some(self.agent_notes && !self.no_agent_notes),
            menu_bar: Some(self.show_menu_bar),
            copy_decorations: Some(self.copy_decorations),
            transparent_background: Some(self.transparent_background && !self.opaque_background),
            color_moved: self.color_moved,
            extensions: Some(!self.no_extensions),
            extension_paths: self
                .extension
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            ..CommonOptions::default()
        }
    }

    fn resolved_sidebar(&self) -> SidebarVisibility {
        if self.sidebar {
            SidebarVisibility::Visible
        } else if self.no_sidebar {
            SidebarVisibility::Hidden
        } else {
            self.sidebar_visibility
        }
    }
}

#[cfg(test)]
mod review_cli_option_tests {
    use super::*;

    #[derive(Debug)]
    struct FakeStartupThemeInput {
        raw: bool,
        chunks: std::collections::VecDeque<Vec<u8>>,
        raw_transitions: Vec<bool>,
    }

    impl FakeStartupThemeInput {
        fn with_response(response: &str) -> Self {
            Self {
                raw: false,
                chunks: std::collections::VecDeque::from([response.as_bytes().to_vec()]),
                raw_transitions: Vec::new(),
            }
        }
    }

    impl ThemeProbeInput for FakeStartupThemeInput {
        fn is_raw(&self) -> Option<bool> {
            Some(self.raw)
        }

        fn set_raw_mode(&mut self, raw: bool) -> std::io::Result<()> {
            self.raw = raw;
            self.raw_transitions.push(raw);
            Ok(())
        }

        fn read_chunk(&mut self, _timeout: Duration) -> std::io::Result<Option<Vec<u8>>> {
            Ok(self.chunks.pop_front())
        }
    }

    #[test]
    fn all_app_startups_with_piped_stdin_open_the_controlling_terminal() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/startup-theme.json"
        ))
        .unwrap();
        let expected = &oracle["expected"]["piped_app"];
        let diff =
            Args::try_parse_from(["workdeck", "diff", "--theme", "github-dark-default"]).unwrap();
        assert_eq!(expected["stdin_is_tty"], false);
        assert_eq!(expected["stdout_is_tty"], true);
        assert_eq!(expected["opens_controlling_terminal"], true);
        assert!(should_open_controlling_terminal(
            diff.command.as_ref(),
            "",
            false,
            true,
        ));
        assert!(!should_open_controlling_terminal(
            diff.command.as_ref(),
            "",
            true,
            true,
        ));
        assert!(!should_open_controlling_terminal(
            diff.command.as_ref(),
            "",
            false,
            false,
        ));

        let pager = Args::try_parse_from(["workdeck", "pager"]).unwrap();
        assert!(should_open_controlling_terminal(
            pager.command.as_ref(),
            "--- a/a\n+++ b/a\n",
            false,
            true,
        ));
        assert!(!should_open_controlling_terminal(
            pager.command.as_ref(),
            "plain pager text\n",
            false,
            true,
        ));
        assert!(should_open_controlling_terminal(None, "", false, true,));
    }

    #[test]
    fn auto_theme_is_probed_before_startup_and_concrete_themes_are_not() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/startup-theme.json"
        ))
        .unwrap();
        let expected = &oracle["expected"]["auto_theme"];
        let mut input = FakeStartupThemeInput::with_response("\x1b]11;rgb:1111/2222/3333\x1b\\");
        let mut output = Vec::new();
        let detected = probe_initial_theme_mode(
            None,
            true,
            Some(&mut input),
            &mut output,
            Duration::from_millis(20),
        )
        .unwrap();
        assert_eq!(detected, Some(TerminalThemeMode::Dark));
        assert_eq!(expected["detected_mode"], "dark");
        assert_eq!(
            output,
            expected["osc_11_query"].as_str().unwrap().as_bytes()
        );
        assert_eq!(input.raw_transitions, [true, false]);

        let mut concrete = FakeStartupThemeInput::with_response("\x1b]11;#ffffff\x07");
        let mut concrete_output = Vec::new();
        assert_eq!(
            probe_initial_theme_mode(
                Some("github-dark-default"),
                true,
                Some(&mut concrete),
                &mut concrete_output,
                Duration::from_millis(20),
            )
            .unwrap(),
            None
        );
        assert!(concrete_output.is_empty());
        assert!(concrete.raw_transitions.is_empty());

        let mut non_terminal = FakeStartupThemeInput::with_response("\x1b]11;#ffffff\x07");
        assert_eq!(
            probe_initial_theme_mode(
                Some("auto"),
                false,
                Some(&mut non_terminal),
                &mut Vec::new(),
                Duration::from_millis(20),
            )
            .unwrap(),
            None
        );
        assert_eq!(
            probe_initial_theme_mode::<FakeStartupThemeInput, Vec<u8>>(
                Some("auto"),
                true,
                None,
                &mut Vec::new(),
                Duration::from_millis(20),
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn extensions_load_before_changesets_and_disabled_startup_executes_none() {
        struct RetirementProbe(std::rc::Rc<std::cell::Cell<u8>>);

        impl Drop for RetirementProbe {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }

        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/startup-extensions.json"
        ))
        .unwrap();
        let order = std::cell::RefCell::new(Vec::new());
        let (extensions, changeset) = load_extensions_before_changeset(
            || {
                order.borrow_mut().push("extensions");
                Ok("prepared")
            },
            || {
                order.borrow_mut().push("changeset");
                Ok("loaded")
            },
        )
        .unwrap();
        assert_eq!(extensions, "prepared");
        assert_eq!(changeset, "loaded");
        assert_eq!(
            serde_json::to_value(order.into_inner()).unwrap(),
            oracle["expected"]["startup_order"]
        );

        let changeset_requested = std::cell::Cell::new(false);
        let extension_error = load_extensions_before_changeset(
            || Err::<(), _>(anyhow::anyhow!("extension startup failed")),
            || {
                changeset_requested.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(extension_error.to_string(), "extension startup failed");
        assert!(!changeset_requested.get());

        let retired = std::rc::Rc::new(std::cell::Cell::new(0));
        let changeset_error = load_extensions_before_changeset(
            || Ok(RetirementProbe(retired.clone())),
            || Err::<(), _>(anyhow::anyhow!("changeset failed")),
        )
        .err()
        .unwrap();
        assert_eq!(changeset_error.to_string(), "changeset failed");
        assert_eq!(retired.get(), 1);

        let directory = tempfile::tempdir().unwrap();
        let mut review = ReviewCliOptions {
            no_extensions: true,
            extension: vec![directory.path().join("must-not-run")],
            startup_notices: vec![StartupNotice::new("config", "config")],
            ..ReviewCliOptions::default()
        };
        let raw_review = review.clone();
        let prepared = load_review_extensions_with_notifications(
            directory.path(),
            &mut review,
            &raw_review,
            ExtensionNotificationHub::new(),
            None,
            None,
        )
        .unwrap();
        assert!(prepared.extensions.is_empty());
        assert!(prepared.application_notices.is_empty());
        assert!(prepared.load_notices.is_empty());
        assert_eq!(prepared.configured_notices, review.startup_notices);
        assert_eq!(
            !review.no_extensions,
            oracle["expected"]["disabled_extension_request"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn distinct_provisional_guards_close_each_owned_load_exactly_once() {
        let first = create_empty_extension_load_result("/repo", ExtensionNotificationHub::new());
        let first_control = first.control.clone();
        let second =
            create_empty_extension_load_result("/recognized", ExtensionNotificationHub::new());
        let second_control = second.control.clone();
        {
            let _first = ProvisionalReviewExtensionLoad::new(first);
            let _second = ProvisionalReviewExtensionLoad::new(second);
        }
        assert_eq!(
            first_control.phase(),
            workdeck_extension_host::ExtensionEventBusPhase::Closed
        );
        assert_eq!(
            second_control.phase(),
            workdeck_extension_host::ExtensionEventBusPhase::Closed
        );
    }

    #[cfg(unix)]
    fn install_external_vcs_test_extension(root: &Path, repo: &Path) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let extension = root.join("custom-vcs");
        std::fs::create_dir_all(&extension).unwrap();
        let factory_log = extension.join("factory.log");
        let executable = extension.join("custom-vcs.sh");
        let quoted_repo = serde_json::to_string(&repo.to_string_lossy()).unwrap();
        let quoted_log = serde_json::to_string(&factory_log.to_string_lossy()).unwrap();
        let script = format!(
            concat!(
                "#!/bin/sh\n",
                "factory_log={quoted_log}\n",
                "printf 'factory\\n' >> \"$factory_log\"\n",
                "while IFS= read -r line; do\n",
                "  request_id=$(printf '%s\\n' \"$line\" | sed -E 's/.*\"id\":([0-9]+).*/\\1/')\n",
                "  case \"$line\" in\n",
                "    *workdeck/handshake*) result='{{\"extension_api_version\":1,\"extension_version\":\"1.0.0\",\"registrations\":[{{\"kind\":\"vcs-adapter\",\"id\":\"custom\",\"name\":\"Custom VCS\",\"operations\":{{\"working-tree-diff\":{{\"watchSignature\":false,\"watchPlan\":false}}}},\"detectionPriority\":50}},{{\"kind\":\"cli-command\",\"name\":\"tools\",\"summary\":\"Tools\"}},{{\"kind\":\"changeset-transform\",\"id\":\"rewrite-title\"}},{{\"kind\":\"file-language\",\"matcher\":{{\"kind\":\"filename\",\"value\":\"ReplacementWorkdeckfile\"}},\"language\":\"ruby\"}},{{\"kind\":\"event-subscription\",\"names\":[\"shutdown\"]}}]}}' ;;\n",
                "    *workdeck/vcs/detect*) result='{{\"id\":\"custom\",\"repoRoot\":{quoted_repo}}}' ;;\n",
                "    *workdeck/vcs/load*) result='{{\"repoRoot\":{quoted_repo},\"sourceLabel\":{quoted_repo},\"title\":\"Custom working copy\",\"patchText\":\"\",\"readFileSource\":false}}' ;;\n",
                "    *workdeck/changeset/transform*) result='{{\"changeset\":{{\"id\":\"changeset:test\",\"source_label\":\"test\",\"title\":\"after\",\"source\":{{\"kind\":\"working-tree\",\"staged\":false}},\"files\":[]}}}}' ;;\n",
                "    *workdeck/shutdown*) printf 'shutdown\\n' >> \"$factory_log\"; exit 0 ;;\n",
                "    *) continue ;;\n",
                "  esac\n",
                "  printf '{{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":%s}}\\n' \"$request_id\" \"$result\"\n",
                "done\n",
            ),
            quoted_log = quoted_log,
            quoted_repo = quoted_repo,
        );
        std::fs::write(&executable, script).unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let manifest = extension.join("workdeck-extension.toml");
        std::fs::write(
            &manifest,
            concat!(
                "id = 'custom-vcs'\n",
                "name = 'Custom VCS'\n",
                "version = '1.0.0'\n",
                "api_version = 1\n",
                "executable = 'custom-vcs.sh'\n",
                "capabilities = ['vcs-adapters', 'cli-commands', 'changeset-transforms', 'file-languages', 'events']\n",
            ),
        )
        .unwrap();
        (manifest, factory_log)
    }

    #[cfg(unix)]
    #[test]
    fn external_vcs_reresolves_repo_config_without_restarting_the_factory() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let nested = repo.join("src/nested");
        std::fs::create_dir_all(repo.join(".custom")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let repo = repo.canonicalize().unwrap();
        let nested = nested.canonicalize().unwrap();
        let (manifest, factory_log) = install_external_vcs_test_extension(directory.path(), &repo);
        let repo_config = directory.path().join("resolved-repo-config.toml");
        std::fs::write(
            &repo_config,
            "[review]\nmode = 'stack'\nexclude_untracked = true\n",
        )
        .unwrap();

        let raw_review = ReviewCliOptions {
            extension: vec![manifest],
            ..ReviewCliOptions::default()
        };
        let mut review = raw_review.clone();
        review.apply_config_defaults(&Config::default());
        review.initial_theme_mode = Some(TerminalThemeMode::Light);
        let resolved_roots = std::cell::RefCell::new(vec![None]);
        let mut prepared = load_review_extensions_with_config_loader(
            &nested,
            &mut review,
            &raw_review,
            ExtensionNotificationHub::new(),
            None,
            None,
            |root| {
                resolved_roots.borrow_mut().push(Some(root.to_owned()));
                Config::load_from_paths(&repo_config, None)
            },
        )
        .unwrap();

        assert_eq!(resolved_roots.into_inner(), [None, Some(repo.clone())]);
        assert!(matches!(review.mode, Some(ReviewLayoutArg::Stack)));
        assert!(review.config_exclude_untracked);
        assert_eq!(review.initial_theme_mode, Some(TerminalThemeMode::Light));
        assert_eq!(std::fs::read_to_string(&factory_log).unwrap(), "factory\n");

        let catalog = compose_review_vcs_catalog(&prepared.extensions);
        let selection = select_review_vcs_adapter(&nested, None, &catalog).unwrap();
        assert_eq!(selection.adapter.id, "custom");
        let input = VcsReviewInput::Diff(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: review.common_options(),
        });
        let loaded =
            load_selected_vcs_changeset(&nested, &selection.adapter, &catalog, &input).unwrap();
        assert_eq!(loaded.repo_root, repo);
        assert_eq!(loaded.changeset.title, "Custom working copy");

        for extension in &mut prepared.extensions {
            extension.retire();
        }
        assert_eq!(
            std::fs::read_to_string(factory_log).unwrap(),
            "factory\nshutdown\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn delegated_extension_cli_load_is_reused_by_both_review_bootstrap_passes() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let nested = repo.join("src/nested");
        std::fs::create_dir_all(repo.join(".custom")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let repo = repo.canonicalize().unwrap();
        let nested = nested.canonicalize().unwrap();
        let (manifest, factory_log) = install_external_vcs_test_extension(directory.path(), &repo);
        let raw_review = ReviewCliOptions {
            extension: vec![manifest],
            ..ReviewCliOptions::default()
        };
        let mut review = raw_review.clone();
        review.apply_config_defaults(&Config::default());
        let notifications = ExtensionNotificationHub::new();

        let previous =
            load_review_extension_pass(&nested, &review, notifications.clone(), None, None, true)
                .unwrap();
        assert_eq!(std::fs::read_to_string(&factory_log).unwrap(), "factory\n");
        let mut prepared = load_review_extensions_with_config_loader(
            &nested,
            &mut review,
            &raw_review,
            notifications,
            Some(previous),
            None,
            |_| Ok(Config::default()),
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&factory_log).unwrap(), "factory\n");
        let catalog = compose_review_vcs_catalog(&prepared.extensions);
        assert_eq!(
            select_review_vcs_adapter(&nested, None, &catalog)
                .unwrap()
                .adapter
                .id,
            "custom"
        );
        for extension in &mut prepared.extensions {
            extension.retire();
        }
        assert_eq!(
            std::fs::read_to_string(factory_log).unwrap(),
            "factory\nshutdown\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn extension_cli_bootstrap_refines_root_reuses_prefix_and_defers_event_bus() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let nested = repo.join("src/nested");
        std::fs::create_dir_all(repo.join(".custom")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let repo = repo.canonicalize().unwrap();
        let nested = nested.canonicalize().unwrap();
        let (manifest, factory_log) = install_external_vcs_test_extension(directory.path(), &repo);

        let mut bootstrap = load_cli_extensions(&nested, &[manifest], false).unwrap();
        assert_eq!(std::fs::read_to_string(&factory_log).unwrap(), "factory\n");
        assert_eq!(
            find_project_root_candidate_with_catalog(&nested, Some(&bootstrap.discovery_catalog)),
            Some(repo)
        );
        let registered = registered_extension_cli_commands(&bootstrap.load.extensions);
        let commands = resolve_extension_cli_commands(&registered);
        assert_eq!(
            find_extension_cli_command("tools", &commands).map(|owner| owner.extension_id.as_str()),
            Some("custom-vcs")
        );
        assert!(commands.collisions.is_empty());
        assert!(bootstrap.load.issues.is_empty());
        assert_eq!(
            bootstrap.load.control.phase(),
            workdeck_extension_host::ExtensionEventBusPhase::Loading
        );
        assert!(bootstrap.load.bind_event_bus());
        assert_eq!(
            bootstrap.load.control.phase(),
            workdeck_extension_host::ExtensionEventBusPhase::Ready
        );

        bootstrap.load.retire();
        assert_eq!(
            std::fs::read_to_string(factory_log).unwrap(),
            "factory\nshutdown\n"
        );
    }

    #[test]
    fn extension_cli_bootstrap_reports_command_collision_without_load_failure() {
        let load = create_empty_extension_load_result("/repo", ExtensionNotificationHub::new());
        let registered = [
            RegisteredExtensionCliCommand {
                extension_index: 0,
                extension_id: "first".into(),
                source_path: PathBuf::from("/first.rs"),
                origin: "config".into(),
                command: workdeck_extension_api::CliCommandRegistration {
                    name: "tools".into(),
                    summary: "first".into(),
                    usage: None,
                },
            },
            RegisteredExtensionCliCommand {
                extension_index: 1,
                extension_id: "second".into(),
                source_path: PathBuf::from("/second.rs"),
                origin: "config".into(),
                command: workdeck_extension_api::CliCommandRegistration {
                    name: "tools".into(),
                    summary: "second".into(),
                    usage: None,
                },
            },
        ];

        let commands = resolve_extension_cli_commands(&registered);
        let collision_issues =
            create_extension_cli_collision_issues(&registered, &commands.collisions);
        assert_eq!(
            find_extension_cli_command("tools", &commands).map(|owner| owner.extension_id.as_str()),
            Some("first")
        );
        assert_eq!(collision_issues[0].extension_id, "second");
        assert!(load.issues.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn configured_session_bootstrap_applies_extensions_and_all_session_fields_before_handoff() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        let nested = repo.join("src/nested");
        std::fs::create_dir_all(repo.join(".custom")).unwrap();
        std::fs::create_dir_all(&nested).unwrap();
        let repo = repo.canonicalize().unwrap();
        let nested = nested.canonicalize().unwrap();
        let (manifest, factory_log) = install_external_vcs_test_extension(directory.path(), &repo);
        let raw_review = ReviewCliOptions {
            extension: vec![manifest],
            ..ReviewCliOptions::default()
        };
        let mut review = raw_review.clone();
        review.apply_config_defaults(&Config::default());
        let mut prepared =
            prepare_review_extensions(&nested, &mut review, &raw_review, None, None).unwrap();
        prepared.vcs_catalog = Some(compose_review_vcs_catalog(&prepared.extensions));
        review.initial_theme_mode = Some(TerminalThemeMode::Dark);
        review.keybindings = vec![UserKeyBindingEntry::new(
            "workdeck.review.nextHunk",
            workdeck_tui::UserKeyBinding::Chord("]".into()),
        )];
        review.view_preferences_config_path = Some(PathBuf::from("/tmp/workdeck-config.toml"));
        let input = CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: review.common_options(),
        });
        let changeset = Changeset {
            id: "changeset:test".into(),
            source_label: "test".into(),
            title: "before".into(),
            summary: None,
            agent_summary: None,
            source: ChangesetSource::WorkingTree { staged: false },
            files: Vec::new(),
        };

        let mut bootstrap = prepare_app_bootstrap(
            &nested,
            Some(repo),
            changeset,
            &review,
            input.clone(),
            None,
            prepared,
        )
        .unwrap();
        assert_eq!(bootstrap.input, input);
        assert_eq!(bootstrap.changeset.title, "after");
        assert_eq!(bootstrap.initial_theme_mode, Some(TerminalThemeMode::Dark));
        assert_eq!(bootstrap.keybindings, review.keybindings);
        assert_eq!(
            bootstrap.view_preferences_config_path.as_deref(),
            Some(Path::new("/tmp/workdeck-config.toml"))
        );
        let prepared = bootstrap.extensions.as_mut().unwrap();
        assert_eq!(prepared.extensions[0].manifest.id, "custom-vcs");
        for extension in &mut prepared.extensions {
            extension.retire();
        }
        assert_eq!(
            std::fs::read_to_string(factory_log).unwrap(),
            "factory\nshutdown\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_session_bootstrap_cannot_mutate_the_active_file_language_registry() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        std::fs::create_dir_all(repo.join(".custom")).unwrap();
        let repo = repo.canonicalize().unwrap();
        let (manifest, _) = install_external_vcs_test_extension(directory.path(), &repo);
        let mut bootstrap = load_cli_extensions(&repo, &[manifest], false).unwrap();
        let mut active = LanguageRegistry::default();
        active.replace_extensions(vec![LanguageRegistration {
            matcher: LanguageMatcher::Filename("CurrentWorkdeckfile".into()),
            language: "python".into(),
            reserved: false,
        }]);

        let attempt: Result<()> = (|| {
            let provisional = build_review_language_registry(&bootstrap.load.extensions);
            assert_eq!(
                provisional.language_for_path("ReplacementWorkdeckfile"),
                "ruby"
            );
            bail!("load failed")
        })();
        assert_eq!(attempt.unwrap_err().to_string(), "load failed");
        assert_eq!(active.language_for_path("CurrentWorkdeckfile"), "python");
        assert_eq!(active.language_for_path("ReplacementWorkdeckfile"), "text");

        bootstrap.load.retire();
    }

    #[test]
    fn frozen_hunk_session_bootstrap_oracle_maps_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/session-bootstrap.json"
        ))
        .unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(
            baselines[0]["source_blob"],
            "eb36b7ed7d63a4dda9621b637b821078a29ee481"
        );
        assert_eq!(baselines[0]["source_bytes"], 4_033);
        assert_eq!(
            baselines[0]["test_blob"],
            "cad35584f7c86a24065396e26c122402b6142a1b"
        );
        assert_eq!(baselines[0]["test_bytes"], 3_604);
        assert_eq!(baselines[0]["passed"], 2);
        assert_eq!(baselines[0]["expect_calls"], 10);
        assert_eq!(
            baselines[1]["source_blob"],
            "2c998dbedaac98efca7f580b6d9505e4862e065e"
        );
        assert_eq!(baselines[1]["source_bytes"], 3_475);
        assert_eq!(
            baselines[1]["test_blob"],
            "06035ce98002460368ffe50ecfe8cd16cd3a617a"
        );
        assert_eq!(baselines[1]["test_bytes"], 2_371);
        assert_eq!(baselines[1]["passed"], 1);
        assert_eq!(baselines[1]["expect_calls"], 6);
        assert_eq!(oracle["test_mapping"].as_array().unwrap().len(), 2);
        assert_eq!(oracle["expected"]["transformed_title"], "after");
        assert_eq!(
            oracle["expected"]["rust_file_language_rollback_mechanism"],
            "session-local registry discarded before commit"
        );
    }

    #[test]
    fn frozen_hunk_extension_cli_bootstrap_oracle_maps_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-cli-bootstrap.json"
        ))
        .unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(baselines[0]["status"], "present");
        assert_eq!(
            baselines[0]["source_blob"],
            "862209d23e2781e2219e6baf02cf2071cba6c6c0"
        );
        assert_eq!(baselines[0]["source_bytes"], 4_307);
        assert_eq!(
            baselines[0]["test_blob"],
            "484208371fd642479b7bb14e9175a1e62a0c70b5"
        );
        assert_eq!(baselines[0]["test_bytes"], 3_228);
        assert_eq!(baselines[0]["passed"], 2);
        assert_eq!(baselines[0]["failed"], 0);
        assert_eq!(baselines[0]["expect_calls"], 7);
        assert_eq!(baselines[1]["status"], "absent");
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 2);
        assert!(mappings.iter().all(|mapping| {
            mapping["source_test"]
                .as_str()
                .is_some_and(|name| !name.is_empty())
                && mapping["rust_tests"]
                    .as_array()
                    .is_some_and(|tests| !tests.is_empty())
        }));
    }

    #[test]
    fn frozen_hunk_extension_bootstrap_oracle_covers_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-bootstrap.json"
        ))
        .unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert_eq!(
            baselines
                .iter()
                .map(|baseline| baseline["commit"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
            ]
        );
        assert!(baselines.iter().all(|baseline| {
            baseline["test_blob"] == "692d87fa76d60a3e8a6093a0c17b70aacd5d65f2"
                && baseline["vcs_test_blob"] == "099b2e0d1176fa0b76d87d379043b8d31b028504"
                && baseline["passed"] == 4
                && baseline["failed"] == 0
                && baseline["expect_calls"] == 18
        }));
        assert_eq!(
            baselines[0]["source_blob"],
            "15adc2d84ebeffe07641a238b8b7e39fc9318fb8"
        );
        assert_eq!(baselines[0]["source_bytes"], 5_662);
        assert_eq!(
            baselines[1]["source_blob"],
            "1a10fc5023e29cf6e32a642b06a3f1f31ad2c72b"
        );
        assert_eq!(baselines[1]["source_bytes"], 5_232);
        assert_eq!(oracle["test_mapping"].as_array().unwrap().len(), 4);
        assert!(
            oracle["test_mapping"]
                .as_array()
                .unwrap()
                .iter()
                .all(|mapping| {
                    mapping["source_test"]
                        .as_str()
                        .is_some_and(|name| !name.is_empty())
                        && mapping["rust_tests"]
                            .as_array()
                            .is_some_and(|tests| !tests.is_empty())
                })
        );
        assert_eq!(
            oracle["expected"]["configuration_roots"],
            serde_json::json!([null, "external-repository"])
        );
        assert_eq!(oracle["expected"]["factory_starts"], 1);
        assert_eq!(oracle["expected"]["preloaded_factory_starts"], 1);
        assert_eq!(oracle["expected"]["final_vcs"], "custom");
        assert_eq!(oracle["expected"]["final_title"], "Custom working copy");
    }

    #[test]
    fn extension_theme_catalog_and_startup_notice_order_match_both_pins() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/startup-extensions.json"
        ))
        .unwrap();
        let config_themes = [NamedCustomThemeConfig {
            id: "ocean".into(),
            accent: Some("#123456".into()),
            ..NamedCustomThemeConfig::default()
        }];
        let extension_themes = [
            RegisteredCustomTheme::new(
                "pack",
                serde_json::json!({"id": "ocean", "accent": "#654321"}),
            ),
            RegisteredCustomTheme::new(
                "pack",
                serde_json::json!({"id": "sunset", "accent": "#abcdef"}),
            ),
            RegisteredCustomTheme::new("pack", serde_json::json!({"id": "Bad Id"})),
        ];
        let themes = collect_session_custom_themes(&config_themes, &extension_themes);
        assert_eq!(
            serde_json::to_value(&themes.themes).unwrap(),
            oracle["expected"]["theme_catalog"]
        );
        assert_eq!(
            themes
                .notices
                .iter()
                .map(|notice| notice.message.as_str())
                .collect::<Vec<_>>(),
            oracle["expected"]["theme_notices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|message| message.as_str().unwrap())
                .collect::<Vec<_>>()
        );

        let mut prepared = PreparedReviewExtensions {
            extensions: Vec::new(),
            notifications: ExtensionNotificationHub::new(),
            pending_trust_repo_root: None,
            configured_notices: vec![StartupNotice::new("config", "config")],
            application_notices: vec![StartupNotice::new("apply", "apply")],
            unknown_vcs_notices: vec![StartupNotice::new("unknown-vcs", "unknown-vcs")],
            load_notices: vec![StartupNotice::new("load", "load")],
            vcs_catalog: None,
        };
        let notices =
            take_review_startup_notices(&mut prepared, vec![StartupNotice::new("theme", "theme")]);
        assert_eq!(
            serde_json::to_value(
                notices
                    .iter()
                    .map(|notice| notice.key.as_str())
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            oracle["expected"]["startup_notice_order"]
        );
    }

    #[test]
    fn common_options_preserve_the_resolved_review_input_for_native_reload() {
        let options = ReviewCliOptions {
            vcs: Some("jj".into()),
            mode: Some(ReviewLayoutArg::Split),
            watch: true,
            pager: true,
            no_line_numbers: true,
            tab_width: Some(8),
            cursor_line: Some(CursorLineArg::Number),
            wrap: true,
            no_hunk_headers: true,
            no_sidebar: true,
            agent_notes: true,
            file_gap: Some(3),
            hunk_gap: Some(2),
            transparent_background: true,
            agent_context: Some(PathBuf::from("notes.json")),
            theme: Some("nord".into()),
            extension: vec![PathBuf::from("review-extension")],
            color_moved: Some(false),
            ..ReviewCliOptions::default()
        };

        let common = options.common_options();
        assert_eq!(common.mode, Some(InputLayoutMode::Split));
        assert_eq!(common.cursor_line, Some(InputCursorLine::Number));
        assert_eq!(common.vcs.as_deref(), Some("jj"));
        assert_eq!(common.theme.as_deref(), Some("nord"));
        assert_eq!(common.agent_context.as_deref(), Some("notes.json"));
        assert_eq!(common.pager, Some(true));
        assert_eq!(common.watch, Some(true));
        assert_eq!(common.line_numbers, Some(false));
        assert_eq!(common.tab_width, Some(8));
        assert_eq!(common.file_gap, Some(3));
        assert_eq!(common.hunk_gap, Some(2));
        assert_eq!(common.wrap_lines, Some(true));
        assert_eq!(common.hunk_headers, Some(false));
        assert_eq!(common.sidebar, Some(SidebarVisibility::Hidden));
        assert_eq!(common.agent_notes, Some(true));
        assert_eq!(common.transparent_background, Some(true));
        assert_eq!(common.color_moved, Some(false));
        assert_eq!(common.extensions, Some(true));
        assert_eq!(common.extension_paths, ["review-extension"]);
    }

    #[test]
    fn automatic_vcs_and_explicit_disable_flags_keep_provider_neutral_defaults() {
        let options = ReviewCliOptions {
            vcs: Some("auto".into()),
            no_watch: true,
            no_extensions: true,
            ..ReviewCliOptions::default()
        };
        let common = options.common_options();
        assert_eq!(common.vcs, None);
        assert_eq!(common.watch, Some(false));
        assert_eq!(common.extensions, Some(false));
    }

    #[test]
    fn custom_native_vcs_ids_are_not_rejected_by_cli_parsing() {
        let parsed =
            Args::try_parse_from(["workdeck", "diff", "--vcs", "fossil-tools", "--no-watch"])
                .unwrap();
        let Some(Command::Diff { review, .. }) = parsed.command else {
            panic!("expected diff command");
        };
        assert_eq!(review.configured_vcs_id(), Some("fossil-tools"));
        assert_eq!(review.preference(), ProviderPreference::Auto);
        assert_eq!(review.common_options().vcs.as_deref(), Some("fossil-tools"));
    }

    #[test]
    fn native_extension_theme_fields_enter_the_provider_neutral_catalog() {
        let registration = workdeck_extension_api::ThemeRegistration {
            id: "midnight-review".into(),
            base: Some("graphite".into()),
            colors: BTreeMap::from([
                ("accent".into(), "#ABCDEF".into()),
                ("panelAlt".into(), "#123456".into()),
            ]),
        };
        let registered = registered_custom_theme("paint.review", &registration);
        let resolved = collect_session_custom_themes(&[], &[registered]);

        assert!(resolved.notices.is_empty());
        assert_eq!(
            serde_json::to_value(&resolved.themes).unwrap(),
            serde_json::json!([{
                "id": "midnight-review",
                "base": "github-dark-default",
                "accent": "#abcdef",
                "panelAlt": "#123456",
            }])
        );
    }

    #[test]
    fn direct_file_mode_is_explicit_and_two_positionals_remain_revisions() {
        let parsed = Args::try_parse_from([
            "workdeck",
            "diff",
            "--files",
            "before.rs",
            "after.rs",
            "--mode",
            "stack",
        ])
        .unwrap();
        let Some(Command::Diff {
            files,
            revisions,
            review,
            ..
        }) = parsed.command
        else {
            panic!("expected diff command");
        };
        assert_eq!(
            files,
            [PathBuf::from("before.rs"), PathBuf::from("after.rs")]
        );
        assert!(revisions.is_empty());
        assert!(matches!(review.mode, Some(ReviewLayoutArg::Stack)));

        let parsed = Args::try_parse_from(["workdeck", "diff", "before.rs", "after.rs"]).unwrap();
        assert!(matches!(
            parsed.command,
            Some(Command::Diff { revisions, files, .. })
                if revisions == ["before.rs", "after.rs"] && files.is_empty()
        ));
    }

    #[test]
    fn direct_file_mode_rejects_malformed_and_mixed_forms() {
        let message = "exactly two file paths";
        for (files, revisions, staged, pathspecs) in [
            (vec!["one"], vec![], false, vec![]),
            (vec!["one", "two", "three"], vec![], false, vec![]),
            (vec!["one", "two"], vec![], true, vec![]),
            (vec!["one", "two"], vec!["target"], false, vec![]),
            (vec!["one", "two"], vec![], false, vec!["src"]),
        ] {
            let files = files.into_iter().map(PathBuf::from).collect::<Vec<_>>();
            let revisions = revisions.into_iter().map(str::to_owned).collect::<Vec<_>>();
            let pathspecs = pathspecs.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert!(
                validate_diff_file_arguments(&files, &revisions, staged, &pathspecs)
                    .unwrap_err()
                    .to_string()
                    .contains(message)
            );
        }
    }

    #[test]
    fn watch_signature_is_captured_before_direct_file_content_changes() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("before.rs"), "before\n").unwrap();
        std::fs::write(directory.path().join("after.rs"), "after\n").unwrap();
        let review = ReviewCliOptions {
            watch: true,
            ..ReviewCliOptions::default()
        };
        let input = CliInput::Files(FileCommandInput {
            left: "before.rs".into(),
            right: "after.rs".into(),
            options: review.common_options(),
        });

        let captured =
            capture_initial_watch_signature(&review, &input, directory.path(), None).unwrap();
        std::fs::write(directory.path().join("after.rs"), "changed and longer\n").unwrap();
        let after = compute_watch_signature(
            &input,
            WatchSignatureContext {
                cwd: directory.path(),
                vcs_catalog: None,
            },
        )
        .unwrap();

        assert_ne!(captured, after);
        let stdin_patch = CliInput::Patch(PatchCommandInput {
            file: None,
            text: None,
            options: review.common_options(),
        });
        assert!(
            capture_initial_watch_signature(&review, &stdin_patch, directory.path(), None)
                .is_none()
        );
    }

    #[test]
    fn configured_review_defaults_match_the_pinned_main_bootstrap_oracle() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/loader-bootstrap.json"
        ))
        .unwrap();
        let expected = &oracle["baselines"][0]["view_defaults"];
        let config = Config::default();
        let review = ReviewCliOptions::from_config(&config);
        let common = review.common_options();
        let tui = review.tui_options();
        let actual = serde_json::json!({
            "mode": "auto",
            "theme": review.theme,
            "lineNumbers": tui.line_numbers,
            "tabWidth": tui.tab_width,
            "fileGap": tui.file_gap,
            "hunkGap": tui.hunk_gap,
            "wrapLines": tui.wrap_lines,
            "hunkHeaders": tui.hunk_headers,
            "menuBar": tui.show_menu_bar,
            "sidebar": "auto",
            "agentNotes": tui.agent_notes,
            "copyDecorations": tui.copy_decorations,
            "cursorLine": "row",
        });

        assert_eq!(actual, *expected);
        assert_eq!(common.menu_bar, Some(true));
        assert_eq!(common.copy_decorations, Some(false));
        assert_eq!(common.sidebar, Some(SidebarVisibility::Auto));

        let mut configured = Config::default();
        configured.review.menu_bar = false;
        configured.review.copy_decorations = true;
        configured.review.sidebar = workdeck_cli::config::ReviewSidebar::Hide;
        let configured_review = ReviewCliOptions::from_config(&configured);
        assert_eq!(
            configured_review.common_options().sidebar,
            Some(SidebarVisibility::Hidden)
        );
        let configured_tui = configured_review.tui_options();
        assert!(!configured_tui.show_menu_bar);
        assert!(configured_tui.copy_decorations);
        assert!(!configured_tui.sidebar);
    }

    #[test]
    fn composed_bootstrap_crosses_the_core_boundary_with_exact_initial_state() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/loader-bootstrap.json"
        ))
        .unwrap();
        let config = Config::default();
        let mut review = ReviewCliOptions::from_config(&config);
        review.initial_theme_mode = Some(TerminalThemeMode::Light);
        let input = CliInput::Patch(PatchCommandInput {
            file: Some("nested/input.patch".into()),
            text: None,
            options: review.common_options(),
        });
        let changeset = parse_patch_input(
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,1 +1,1 @@\n-old\n+new\n",
            "nested/input.patch",
        )
        .unwrap();
        let bootstrap = build_app_bootstrap(
            Path::new("/repo/subdir"),
            Some(PathBuf::from("/repo")),
            changeset,
            &review,
            input.clone(),
            Some("initial-signature".into()),
            PreparedReviewExtensions {
                extensions: Vec::new(),
                notifications: ExtensionNotificationHub::new(),
                pending_trust_repo_root: None,
                configured_notices: vec![StartupNotice::new("fixture", "notice")],
                application_notices: Vec::new(),
                unknown_vcs_notices: Vec::new(),
                load_notices: Vec::new(),
                vcs_catalog: Some(bundled_vcs_catalog().clone()),
            },
        );
        let actual = serde_json::json!({
            "mode": match bootstrap.initial_mode {
                InputLayoutMode::Auto => "auto",
                InputLayoutMode::Split => "split",
                InputLayoutMode::Stack => "stack",
            },
            "theme": bootstrap.initial_theme,
            "lineNumbers": bootstrap.initial_show_line_numbers,
            "tabWidth": bootstrap.initial_tab_width,
            "fileGap": bootstrap.initial_file_gap,
            "hunkGap": bootstrap.initial_hunk_gap,
            "wrapLines": bootstrap.initial_wrap_lines,
            "hunkHeaders": bootstrap.initial_show_hunk_headers,
            "menuBar": bootstrap.initial_show_menu_bar,
            "sidebar": match bootstrap.initial_sidebar {
                SidebarVisibility::Auto => serde_json::Value::String("auto".into()),
                SidebarVisibility::Visible => serde_json::Value::Bool(true),
                SidebarVisibility::Hidden => serde_json::Value::Bool(false),
            },
            "agentNotes": bootstrap.initial_show_agent_notes,
            "copyDecorations": bootstrap.initial_copy_decorations,
            "cursorLine": match bootstrap.initial_cursor_line {
                InputCursorLine::Row => "row",
                InputCursorLine::Number => "number",
                InputCursorLine::Off => "off",
            },
        });

        assert_eq!(actual, oracle["baselines"][0]["view_defaults"]);
        assert_eq!(bootstrap.initial_theme_mode, Some(TerminalThemeMode::Light));
        assert_eq!(bootstrap.input, input);
        assert_eq!(bootstrap.reload_context.cwd, Path::new("/repo/subdir"));
        assert_eq!(
            bootstrap.reload_context.repo_root.as_deref(),
            Some(Path::new("/repo"))
        );
        assert_eq!(
            bootstrap.reload_context.initial_watch_signature.as_deref(),
            Some("initial-signature")
        );
        assert!(bootstrap.reload_context.vcs_catalog.is_some());
        assert_eq!(bootstrap.startup_notices[0].key, "fixture");
        let startup_oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/startup-extensions.json"
        ))
        .unwrap();
        assert_eq!(
            bootstrap.extensions.is_some(),
            startup_oracle["expected"]["bootstrap_owns_extensions"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn composed_bootstrap_carries_config_themes_and_view_preference_destination() {
        let directory = tempfile::tempdir().unwrap();
        let user_config = directory.path().join("config.toml");
        std::fs::write(
            &user_config,
            concat!(
                "[ui]\ntheme = 'custom'\n",
                "[custom_theme]\nbase = 'catppuccin-mocha'\naccent = '#7755aa'\n",
                "[custom_theme.syntax_scopes]\ncomment = '#998877'\n",
            ),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("before.ts"),
            "export const alpha = 1;\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("after.ts"),
            "export const alpha = 2;\n",
        )
        .unwrap();
        let config = Config::load_from_paths(
            &directory.path().join("missing-repo-config.toml"),
            Some(&user_config),
        )
        .unwrap();
        let review = ReviewCliOptions::from_config(&config);
        let input = CliInput::Files(FileCommandInput {
            left: "before.ts".into(),
            right: "after.ts".into(),
            options: review.common_options(),
        });
        let changeset = load_file_comparison(
            directory.path(),
            Path::new("before.ts"),
            Path::new("after.ts"),
        )
        .unwrap();
        let bootstrap = build_app_bootstrap(
            directory.path(),
            None,
            changeset,
            &review,
            input,
            None,
            PreparedReviewExtensions {
                extensions: Vec::new(),
                notifications: ExtensionNotificationHub::new(),
                pending_trust_repo_root: None,
                configured_notices: Vec::new(),
                application_notices: Vec::new(),
                unknown_vcs_notices: Vec::new(),
                load_notices: Vec::new(),
                vcs_catalog: None,
            },
        );

        assert_eq!(bootstrap.initial_theme.as_deref(), Some("custom"));
        assert_eq!(
            serde_json::to_value(&bootstrap.custom_themes).unwrap(),
            serde_json::json!([{
                "id": "custom",
                "base": "catppuccin-mocha",
                "accent": "#7755aa",
                "syntaxScopes": { "comment": "#998877" },
            }]),
        );
        assert_eq!(
            bootstrap.view_preferences_config_path.as_deref(),
            Some(user_config.as_path())
        );
    }

    #[test]
    fn prepared_direct_file_bootstrap_applies_relative_agent_context_before_handoff() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("before.ts"),
            "export const answer = 41;\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("after.ts"),
            "export const answer = 42;\nexport const bonus = true;\n",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("agent.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "summary": "Agent added the bonus export.",
                "files": [{
                    "path": "after.ts",
                    "annotations": [{
                        "newRange": [2, 2],
                        "summary": "Introduces the bonus flag."
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let review = ReviewCliOptions {
            agent_context: Some(PathBuf::from("agent.json")),
            ..ReviewCliOptions::from_config(&Config::default())
        };
        let input = CliInput::Files(FileCommandInput {
            left: "before.ts".into(),
            right: "after.ts".into(),
            options: review.common_options(),
        });
        let changeset = load_file_comparison(
            directory.path(),
            Path::new("before.ts"),
            Path::new("after.ts"),
        )
        .unwrap();
        let bootstrap = prepare_app_bootstrap(
            directory.path(),
            None,
            changeset,
            &review,
            input,
            None,
            PreparedReviewExtensions {
                extensions: Vec::new(),
                notifications: ExtensionNotificationHub::new(),
                pending_trust_repo_root: None,
                configured_notices: Vec::new(),
                application_notices: Vec::new(),
                unknown_vcs_notices: Vec::new(),
                load_notices: Vec::new(),
                vcs_catalog: None,
            },
        )
        .unwrap();

        assert_eq!(
            bootstrap.changeset.agent_summary.as_deref(),
            Some("Agent added the bonus export.")
        );
        assert_eq!(
            bootstrap.changeset.files[0]
                .agent
                .as_ref()
                .unwrap()
                .annotations[0]
                .summary,
            "Introduces the bonus flag."
        );
    }

    #[test]
    fn unknown_vcs_ids_fall_back_with_a_sanitized_startup_notice() {
        let root = tempfile::tempdir().unwrap();
        let selection = select_review_vcs_adapter(
            root.path(),
            Some("fossil\u{1b}[31m-tools"),
            bundled_vcs_catalog(),
        )
        .unwrap();
        assert_eq!(selection.adapter.id, "git");
        let notice = selection.unknown_id_notice.unwrap();
        assert_eq!(notice.key, "vcs:unknown:fossil\u{1b}[31m-tools");
        assert!(notice.message.contains("Unknown vcs \"fossil-tools\""));
        assert!(notice.message.contains("falling back to git"));
        assert!(!notice.message.contains('\u{1b}'));
    }

    #[test]
    fn adapter_patch_repo_root_overrides_built_in_discovery_for_the_session() {
        let cwd = Path::new("/checkout/nested");
        let adapter_root = Path::new("/extension/repository");
        assert_eq!(
            resolve_review_repo_root(cwd, ProviderPreference::Auto, Some(adapter_root)),
            adapter_root
        );
    }

    #[test]
    fn user_command_bindings_flow_from_config_into_review_options() {
        let config = Config {
            keybindings: vec![UserKeyBindingEntry::new(
                "workdeck.app.quit",
                workdeck_tui::UserKeyBinding::Chord("ctrl+q".into()),
            )],
            keybinding_notices: vec!["ignored invalid binding".into()],
            ..Config::default()
        };

        let review = ReviewCliOptions::from_config(&config);
        let tui = review.tui_options();
        assert_eq!(tui.keybindings, config.keybindings);
        assert_eq!(tui.keybinding_notices, config.keybinding_notices);
    }

    #[test]
    fn extension_configuration_flows_from_config_into_review_startup() {
        let mut config = Config::default();
        config.resolved_extensions.extension_configs.insert(
            "example.review".into(),
            serde_json::json!({ "threshold": 3 }),
        );
        config.resolved_extensions.enabled = false;
        config.startup_notices.push(StartupNotice::new(
            "extension:repo-config:example.review",
            "Repo config overrides settings for extension(s): example.review",
        ));

        let review = ReviewCliOptions::from_config(&config);
        assert_eq!(
            review.extension_config["example.review"],
            serde_json::json!({ "threshold": 3 })
        );
        assert!(review.no_extensions);
        assert_eq!(review.startup_notices, config.startup_notices);
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/config-extensions.json"
        ))
        .unwrap();
        assert_eq!(
            !review.no_extensions,
            oracle["expected"]["precedence"]["hardOff"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn explicit_no_extensions_remains_a_hard_off_after_enabled_config() {
        let config = Config::default();
        assert!(config.resolved_extensions.enabled);
        let mut review = ReviewCliOptions {
            no_extensions: true,
            ..ReviewCliOptions::default()
        };

        review.apply_config_defaults(&config);

        assert!(review.no_extensions);
    }
}

#[cfg(test)]
mod extension_cli_tests {
    use super::*;

    #[test]
    fn unknown_top_level_tokens_retain_raw_args_for_native_extensions() {
        let parsed = Args::try_parse_from([
            "workdeck",
            "--extension",
            "./cli-tools",
            "cli-tools",
            "review",
            "--mode",
            "split",
            "--",
            "-leading",
        ])
        .unwrap();
        assert_eq!(parsed.extension, [PathBuf::from("./cli-tools")]);
        assert!(matches!(
            parsed.command,
            Some(Command::External(tokens))
                if tokens == ["cli-tools", "review", "--mode", "split", "--", "-leading"]
        ));
    }

    #[test]
    fn global_extension_paths_apply_to_built_in_review_commands() {
        let mut parsed = Args::try_parse_from([
            "workdeck",
            "diff",
            "--extension",
            "./one",
            "--extension=./two",
        ])
        .unwrap();
        let review = parsed
            .command
            .as_mut()
            .and_then(Command::review_options_mut)
            .unwrap();
        review.extension.clone_from(&parsed.extension);
        assert_eq!(
            review.extension,
            [PathBuf::from("./one"), PathBuf::from("./two")]
        );
    }

    #[test]
    fn invalid_and_duplicate_manifests_are_contained_before_process_start() {
        let root = tempfile::tempdir().unwrap();
        let paths = ["first", "duplicate", "invalid"]
            .map(|name| root.path().join(name).join("workdeck-extension.toml"));
        for path in &paths {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        let valid = "id = 'example'\nname = 'Example'\nversion = '1.0.0'\napi_version = 1\nexecutable = 'example'\ncapabilities = []\n";
        std::fs::write(&paths[0], valid).unwrap();
        std::fs::write(&paths[1], valid).unwrap();
        std::fs::write(&paths[2], "not valid = [").unwrap();

        let candidates = paths
            .iter()
            .cloned()
            .map(|path| workdeck_extension_host::ManifestCandidate {
                path,
                origin: workdeck_extension_host::ManifestOrigin::Explicit,
            })
            .collect::<Vec<_>>();
        let prepared = workdeck_extension_host::prepare_extension_load(
            workdeck_extension_host::LoadExtensionsOptions {
                candidates: &candidates,
                cwd: root.path(),
                all_candidates: None,
                previous_load: None,
                host_version: "test",
                extension_configs: &BTreeMap::new(),
                notifications: None,
                pending_trust_repo_root: None,
            },
        );
        assert_eq!(prepared.accepted.len(), 1);
        assert_eq!(prepared.accepted[0].manifest_path, paths[0]);
        assert_eq!(prepared.result.issues.len(), 2);
        assert!(
            prepared
                .result
                .issues
                .iter()
                .any(|issue| issue.message.contains("already loaded"))
        );
        assert!(
            prepared
                .result
                .issues
                .iter()
                .any(|issue| issue.message.contains("invalid extension manifest"))
        );
    }

    #[test]
    fn delegated_parsing_preserves_bootstrap_flags_and_rejects_extension_targets() {
        let parsed = parse_delegated_args(
            Path::new("/tmp/repo"),
            &[PathBuf::from("tool")],
            false,
            vec!["diff".into(), "HEAD".into()],
        )
        .unwrap()
        .unwrap();
        assert_eq!(parsed.cwd, PathBuf::from("/tmp/repo"));
        assert_eq!(parsed.extension, [PathBuf::from("tool")]);
        assert!(
            matches!(parsed.command, Some(Command::Diff { revisions, .. }) if revisions == ["HEAD"])
        );

        let parsed =
            parse_delegated_args(Path::new("."), &[], false, vec!["another-extension".into()])
                .unwrap()
                .unwrap();
        assert!(matches!(parsed.command, Some(Command::External(_))));
    }

    #[test]
    fn extension_metadata_is_collapsed_to_one_terminal_safe_line() {
        assert_eq!(
            extension_cli_commands::single_line("safe\n\x1b]52;c;AAAA\x07\ttext"),
            "safetext"
        );
    }

    #[test]
    fn bootstrap_flags_are_rejected_by_non_review_built_ins() {
        let parsed = Args::try_parse_from(["workdeck", "--extension", "tool", "status"]).unwrap();
        assert!(
            run(parsed)
                .unwrap_err()
                .to_string()
                .contains("may be used only")
        );
    }
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    #[command(about = "Print repo config path")]
    Path {
        #[arg(long, help = "Print config path as JSON")]
        json: bool,
    },
    #[command(about = "Print merged config")]
    Show {
        #[arg(long, help = "Print config as JSON")]
        json: bool,
    },
    #[command(about = "Initialize repo config")]
    Init {
        #[arg(long, help = "Print init result as JSON")]
        json: bool,
    },
    #[command(about = "Validate merged config")]
    Validate {
        #[arg(long, help = "Print validation result as JSON")]
        json: bool,
    },
    #[command(about = "Get one repo config key")]
    Get {
        key: String,
        #[arg(long, help = "Print value as JSON")]
        json: bool,
    },
    #[command(about = "Set one repo config key")]
    Set {
        key: String,
        value: String,
        #[arg(long, help = "Print set result as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum EventsCommand {
    #[command(about = "List event log records")]
    List {
        #[arg(long, help = "Print events as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum FilesCommand {
    #[command(about = "List direct repo file browser entries")]
    List {
        path: Option<PathBuf>,
        #[arg(long, help = "Print file entries as JSON")]
        json: bool,
    },
    #[command(about = "Show a file preview")]
    Show {
        path: PathBuf,
        #[arg(long, help = "Print file preview as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ChangesCommand {
    #[command(about = "List changed files")]
    List {
        #[arg(
            long,
            default_value = "directory",
            help = "Group by directory or status"
        )]
        group: String,
        #[arg(long, help = "Print changes as JSON")]
        json: bool,
    },
    #[command(about = "Show staged and unstaged diff preview for a path")]
    Diff {
        path: PathBuf,
        #[arg(long, help = "Print diff preview as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum IssueCommand {
    #[command(about = "List local issues")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long, help = "Print issues as JSON")]
        json: bool,
    },
    #[command(about = "Create a local issue")]
    Create {
        title: Option<String>,
        #[arg(
            long,
            value_name = "PATH",
            help = "Read issue fields from JSON file, or '-' for stdin"
        )]
        from_json: Option<PathBuf>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long, value_delimiter = ',')]
        label: Vec<String>,
        #[arg(long = "commit", value_delimiter = ',')]
        linked_commit: Vec<String>,
        #[arg(long = "file")]
        linked_file: Vec<String>,
        #[arg(long, help = "Print the created issue as JSON")]
        json: bool,
    },
    #[command(about = "Update a local issue")]
    Update {
        key: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        cycle: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
        #[arg(long)]
        due_at: Option<String>,
        #[arg(long, value_delimiter = ',')]
        label: Vec<String>,
        #[arg(long = "commit", value_delimiter = ',')]
        linked_commit: Vec<String>,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Link a file path to an issue")]
    Link {
        key: String,
        path: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(name = "link-file", about = "Link a file path to an issue")]
    LinkFile {
        key: String,
        path: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(
        name = "unlink-file",
        about = "Remove a linked file path from an issue"
    )]
    UnlinkFile {
        key: String,
        path: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(name = "link-commit", about = "Link a commit SHA to an issue")]
    LinkCommit {
        key: String,
        sha: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(
        name = "unlink-commit",
        about = "Remove a linked commit SHA from an issue"
    )]
    UnlinkCommit {
        key: String,
        sha: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Close an issue")]
    Close {
        key: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Reopen an issue as Todo")]
    Reopen {
        key: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Move an issue to a status")]
    Move {
        key: String,
        #[arg(long)]
        status: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Assign an issue")]
    Assign {
        key: String,
        assignee: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Unassign an issue")]
    Unassign {
        key: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Manage issue labels")]
    Label {
        #[command(subcommand)]
        command: IssueLabelCommand,
    },
    #[command(about = "Delete an issue")]
    Delete {
        key: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Print deletion result as JSON")]
        json: bool,
    },
    #[command(about = "Show one local issue")]
    Show {
        key: String,
        #[arg(long, help = "Print the issue as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum IssueLabelCommand {
    #[command(about = "Add a label to an issue")]
    Add {
        key: String,
        label: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
    #[command(about = "Remove a label from an issue")]
    Remove {
        key: String,
        label: String,
        #[arg(long, help = "Print the updated issue as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum AgentCommand {
    #[command(about = "List local agent sessions")]
    List {
        #[arg(long, help = "Print sessions as JSON")]
        json: bool,
    },
    #[command(about = "Record a local agent session")]
    Record {
        title: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long = "plan")]
        plan_item: Vec<String>,
        #[arg(long = "file")]
        touched_file: Vec<String>,
        #[arg(long = "command")]
        command_run: Vec<String>,
        #[arg(long = "test")]
        test_run: Vec<String>,
        #[arg(long = "note")]
        handoff_note: Vec<String>,
        #[arg(long, help = "Print the recorded session as JSON")]
        json: bool,
    },
    #[command(about = "Show one agent session")]
    Show {
        id: String,
        #[arg(long, help = "Print the session as JSON")]
        json: bool,
    },
    #[command(about = "Update a local agent session")]
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        goal: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(about = "Mark an agent session done")]
    Finish {
        id: String,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "append-plan", about = "Append a plan item to an agent session")]
    AppendPlan {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-file", about = "Append a touched file to an agent session")]
    AddFile {
        id: String,
        path: String,
        #[arg(long, default_value = "modified")]
        change_type: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-command", about = "Append a command to an agent session")]
    AddCommand {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-test", about = "Append a test command to an agent session")]
    AddTest {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(name = "add-note", about = "Append a handoff note to an agent session")]
    AddNote {
        id: String,
        text: String,
        #[arg(long, help = "Print the updated session as JSON")]
        json: bool,
    },
    #[command(about = "Delete an agent session")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Print deletion result as JSON")]
        json: bool,
    },
    #[command(about = "Import agent sessions from JSON or JSONL")]
    Import {
        path: PathBuf,
        #[arg(long, help = "Print imported sessions as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    #[command(about = "List local projects")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print projects as JSON")]
        json: bool,
    },
    #[command(about = "Create or update a local project")]
    Save {
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print the saved project as JSON")]
        json: bool,
    },
    #[command(about = "Show one project")]
    Show {
        id: String,
        #[arg(long, help = "Print the project as JSON")]
        json: bool,
    },
    #[command(about = "Delete a project")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Clear issue references")]
        force: bool,
        #[arg(long, help = "Print the deleted project as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum CycleCommand {
    #[command(about = "List local cycles")]
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print cycles as JSON")]
        json: bool,
    },
    #[command(about = "Create or update a local cycle")]
    Save {
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        starts_at: Option<String>,
        #[arg(long)]
        ends_at: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long, help = "Print the saved cycle as JSON")]
        json: bool,
    },
    #[command(about = "Show one cycle")]
    Show {
        id: String,
        #[arg(long, help = "Print the cycle as JSON")]
        json: bool,
    },
    #[command(about = "Delete a cycle")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Clear issue references")]
        force: bool,
        #[arg(long, help = "Print the deleted cycle as JSON")]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum LabelCommand {
    #[command(about = "List local labels")]
    List {
        #[arg(long)]
        color: Option<String>,
        #[arg(long, help = "Print labels as JSON")]
        json: bool,
    },
    #[command(about = "Create or update a local label")]
    Save {
        name: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        color: Option<String>,
        #[arg(long, help = "Print the saved label as JSON")]
        json: bool,
    },
    #[command(about = "Show one label")]
    Show {
        id: String,
        #[arg(long, help = "Print the label as JSON")]
        json: bool,
    },
    #[command(about = "Delete a label")]
    Delete {
        id: String,
        #[arg(long, help = "Confirm deletion")]
        yes: bool,
        #[arg(long, help = "Remove label from issues")]
        force: bool,
        #[arg(long, help = "Print the deleted label as JSON")]
        json: bool,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("command exited with status {0}")]
struct CommandExit(i32);

fn main() -> ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(code.try_into().unwrap_or(2));
        }
    };
    let wants_json = args.wants_json();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if error.downcast_ref::<CommandExit>().is_some()
                || is_json_error_already_printed(&error)
            {
                // The command printed a structured JSON error with command-specific details.
            } else if wants_json {
                let _ = print_json_error(&error);
            } else {
                eprintln!("Error: {error:#}");
            }
            ExitCode::from(classify_exit_code(&error))
        }
    }
}

fn run(args: Args) -> Result<()> {
    run_with_preloaded_extensions(args, None)
}

fn run_with_preloaded_extensions(
    mut args: Args,
    mut preloaded_extensions: Option<PreloadedExtensionBootstrap>,
) -> Result<()> {
    let has_extension_bootstrap_flags =
        !args.extension.is_empty() || args.extensions || args.no_extensions;
    let accepts_extension_bootstrap = args.command.as_ref().is_none_or(|command| {
        command.is_review_command() || matches!(command, Command::External(_))
    });
    if has_extension_bootstrap_flags && !accepts_extension_bootstrap {
        bail!(
            "`--extension`, `--extensions`, and `--no-extensions` may be used only with a Workdeck review command or an extension CLI command"
        );
    }

    if let Some(review) = args.command.as_mut().and_then(Command::review_options_mut) {
        review.extension.clone_from(&args.extension);
        review.no_extensions = args.no_extensions && !args.extensions;
    }

    // Spool piped review input and attach the controlling terminal before config, theme, or
    // extension initialization can create Crossterm's process-global event reader.
    let mut prepared_piped_input = prepare_piped_review_input(args.command.as_ref())?;

    if matches!(args.command, Some(Command::External(_))) {
        let Command::External(tokens) = args.command.take().expect("external command was present")
        else {
            unreachable!()
        };
        let (command_name, command_args) = tokens
            .split_first()
            .context("extension CLI command is missing its command name")?;
        return handle_extension_cli_command(
            &args.cwd,
            &args.extension,
            args.no_extensions && !args.extensions,
            command_name,
            command_args,
        );
    }

    if !args.init
        && !args.status_json
        && args
            .command
            .as_ref()
            .is_some_and(Command::is_review_command)
    {
        let mut command = args.command.take().expect("review command was present");
        let raw_review = command
            .review_options()
            .expect("review command has review options")
            .clone();
        let delegated_discovery_catalog = preloaded_extensions
            .as_ref()
            .map(|preloaded| preloaded.discovery_catalog.clone());
        let config_catalog = delegated_discovery_catalog
            .as_ref()
            .unwrap_or_else(|| bundled_vcs_catalog());
        let config_root = find_project_root_candidate_with_catalog(&args.cwd, Some(config_catalog))
            .unwrap_or_else(|| args.cwd.clone());
        let config = Config::load(&config_root)?;
        command
            .review_options_mut()
            .expect("review command has review options")
            .apply_config_defaults(&config);
        let initial_theme_mode = detect_initial_review_theme_mode(
            command
                .review_options()
                .expect("review command has review options"),
            prepared_piped_input
                .as_mut()
                .and_then(|prepared| prepared.terminal.as_mut()),
        )?;
        command
            .review_options_mut()
            .expect("review command has review options")
            .initial_theme_mode = initial_theme_mode;
        return handle_review_command(
            &args.cwd,
            command,
            raw_review,
            preloaded_extensions.take().map(|preloaded| preloaded.load),
            delegated_discovery_catalog,
            prepared_piped_input,
        );
    }

    if !args.init
        && !args.status_json
        && args
            .command
            .as_ref()
            .is_some_and(Command::is_global_command)
    {
        return handle_global_command(
            &args.cwd,
            args.command.take().expect("global command was present"),
        );
    }

    let repo_root = git::discover_repo_root(&args.cwd)?;

    if let Some(Command::Doctor { json }) = &args.command {
        handle_doctor(&repo_root, *json)?;
        return Ok(());
    }

    let config = Config::load(&repo_root)?;
    let store = WorkdeckStore::new(config.data_dir(&repo_root));

    if args.init {
        store.init()?;
        println!("initialized {}", store.root().display());
        return Ok(());
    }

    if args.status_json {
        print_status(&repo_root, true)?;
        return Ok(());
    }

    if let Some(command) = args.command {
        match command {
            Command::Status { json } => print_status(&repo_root, json)?,
            Command::Diff { .. }
            | Command::Show { .. }
            | Command::Stash { .. }
            | Command::Patch { .. }
            | Command::Difftool { .. }
            | Command::Pager { .. } => {
                unreachable!("review commands are handled before config load")
            }
            Command::Session { .. }
            | Command::Extension { .. }
            | Command::Migrate { .. }
            | Command::Markup { .. }
            | Command::Skill { .. }
            | Command::Update { .. } => {
                unreachable!("global commands are handled before repository config load")
            }
            Command::Files { command } => handle_files_command(&repo_root, command)?,
            Command::Changes { command } => handle_changes_command(&repo_root, command)?,
            Command::Search {
                query,
                target,
                json,
            } => handle_search_command(&repo_root, &config, &store, query, target, json)?,
            Command::Config { command } => handle_config_command(&repo_root, &store, command)?,
            Command::Events { command } => handle_events_command(&store, command)?,
            Command::Import {
                path,
                merge,
                replace,
                dry_run,
                json,
            } => handle_import_command(&store, path, merge, replace, dry_run, json)?,
            Command::Doctor { .. } => unreachable!("doctor is handled before config load"),
            Command::Export { json, jsonl } => handle_export(&repo_root, &store, json, jsonl)?,
            Command::Issue { command } => handle_issue_command(&store, command)?,
            Command::Agent { command } => handle_agent_command(&store, command)?,
            Command::Project { command } => handle_project_command(&store, command)?,
            Command::Cycle { command } => handle_cycle_command(&store, command)?,
            Command::Label { command } => handle_label_command(&store, command)?,
            Command::External(_) => {
                unreachable!("extension CLI commands are handled before repository discovery")
            }
        }
        return Ok(());
    }
    let raw_review = ReviewCliOptions {
        extension: args.extension,
        no_extensions: args.no_extensions && !args.extensions,
        ..ReviewCliOptions::default()
    };
    let mut review = raw_review.clone();
    review.apply_config_defaults(&config);
    review.initial_theme_mode = detect_initial_review_theme_mode(
        &review,
        prepared_piped_input
            .as_mut()
            .and_then(|prepared| prepared.terminal.as_mut()),
    )?;
    let mut prepared_extensions = prepare_review_extensions(
        &args.cwd,
        &mut review,
        &raw_review,
        preloaded_extensions.take().map(|preloaded| preloaded.load),
        None,
    )?;
    let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
    prepared_extensions.vcs_catalog = Some(catalog.clone());
    let mut vcs_input = VcsReviewInput::Diff(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: {
            let mut options = review.common_options();
            options.exclude_untracked = Some(review.config_exclude_untracked);
            options
        },
    });
    let selection = select_review_vcs_adapter(&args.cwd, review.configured_vcs_id(), &catalog)?;
    if let Some(notice) = selection.unknown_id_notice {
        prepared_extensions.unknown_vcs_notices.push(notice);
    }
    let adapter = selection.adapter;
    vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
    let session_input = match &vcs_input {
        VcsReviewInput::Diff(input) => CliInput::Vcs(input.clone()),
        _ => unreachable!(),
    };
    let initial_watch_signature =
        capture_initial_watch_signature(&review, &session_input, &args.cwd, Some(&catalog));
    let loaded = load_selected_vcs_changeset(&args.cwd, &adapter, &catalog, &vcs_input)?;
    if !loaded.changeset.is_empty() {
        let mut reload = || {
            load_selected_vcs_changeset(&args.cwd, &adapter, &catalog, &vcs_input)
                .map(|loaded| loaded.changeset)
        };
        run_review_with_preloaded_extensions(
            &args.cwd,
            LoadedReviewChangeset {
                changeset: loaded.changeset,
                repo_root: Some(loaded.repo_root),
            },
            review,
            session_input,
            initial_watch_signature,
            Some(&mut reload),
            prepared_extensions,
        )
    } else {
        let app = App::new(&args.cwd)?;
        workdeck_cli::tui::run(app)
    }
}

impl Args {
    fn wants_json(&self) -> bool {
        self.status_json || self.command.as_ref().is_some_and(Command::wants_json)
    }
}

impl Command {
    fn is_review_command(&self) -> bool {
        matches!(
            self,
            Command::Diff { .. }
                | Command::Show { .. }
                | Command::Stash { .. }
                | Command::Patch { .. }
                | Command::Difftool { .. }
                | Command::Pager { .. }
        )
    }

    fn review_options(&self) -> Option<&ReviewCliOptions> {
        match self {
            Self::Diff { review, .. }
            | Self::Show { review, .. }
            | Self::Patch { review, .. }
            | Self::Difftool { review, .. }
            | Self::Pager { review } => Some(review),
            Self::Stash {
                command: StashCommand::Show { review, .. },
            } => Some(review),
            _ => None,
        }
    }

    fn review_options_mut(&mut self) -> Option<&mut ReviewCliOptions> {
        match self {
            Self::Diff { review, .. }
            | Self::Show { review, .. }
            | Self::Patch { review, .. }
            | Self::Difftool { review, .. }
            | Self::Pager { review } => Some(review),
            Self::Stash {
                command: StashCommand::Show { review, .. },
            } => Some(review),
            _ => None,
        }
    }

    fn is_global_command(&self) -> bool {
        matches!(
            self,
            Command::Session { .. }
                | Command::Extension { .. }
                | Command::Migrate { .. }
                | Command::Markup { .. }
                | Command::Skill { .. }
                | Command::Update { .. }
        )
    }

    fn wants_json(&self) -> bool {
        match self {
            Command::Diff { .. }
            | Command::Show { .. }
            | Command::Stash { .. }
            | Command::Patch { .. }
            | Command::Difftool { .. }
            | Command::Pager { .. } => false,
            Command::Session { command } => command.wants_json(),
            Command::Extension { command } => command.wants_json(),
            Command::Migrate { command } => command.wants_json(),
            Command::Markup { command } => command.wants_json(),
            Command::Skill { command } => command.wants_json(),
            Command::Update { .. } => false,
            Command::Status { json } => *json,
            Command::Files { command } => command.wants_json(),
            Command::Changes { command } => command.wants_json(),
            Command::Search { json, .. } => *json,
            Command::Config { command } => command.wants_json(),
            Command::Events { command } => command.wants_json(),
            Command::Import { json, .. } => *json,
            Command::Doctor { json } => *json,
            Command::Export { json, jsonl } => *json && !*jsonl,
            Command::Issue { command } => command.wants_json(),
            Command::Agent { command } => command.wants_json(),
            Command::Project { command } => command.wants_json(),
            Command::Cycle { command } => command.wants_json(),
            Command::Label { command } => command.wants_json(),
            Command::External(_) => false,
        }
    }
}

impl LiveSessionCommand {
    fn wants_json(&self) -> bool {
        match self {
            Self::List { json }
            | Self::Get { json, .. }
            | Self::Context { json, .. }
            | Self::Review { json, .. }
            | Self::Reload { json, .. }
            | Self::Navigate { json, .. }
            | Self::Quit { json, .. } => *json,
            Self::Comment { command } => command.wants_json(),
        }
    }
}

impl LiveCommentCommand {
    fn wants_json(&self) -> bool {
        match self {
            Self::Add { json, .. } | Self::List { json, .. } | Self::Remove { json, .. } => *json,
        }
    }
}

impl ExtensionCommand {
    fn wants_json(&self) -> bool {
        match self {
            Self::Install { json, .. }
            | Self::List { json }
            | Self::Update { json, .. }
            | Self::Remove { json, .. }
            | Self::Validate { json, .. }
            | Self::Trust { json, .. } => *json,
        }
    }
}

impl MigrateCommand {
    fn wants_json(&self) -> bool {
        match self {
            Self::Hunk { json, .. } => *json,
        }
    }
}

impl MarkupCommand {
    fn wants_json(&self) -> bool {
        matches!(self, Self::Render { json: true, .. })
    }
}

impl SkillCommand {
    fn wants_json(&self) -> bool {
        matches!(self, Self::Path { json: true, .. })
    }
}

struct PreparedPipedInput {
    text: String,
    terminal: Option<workdeck_tui::ControllingTerminal<File>>,
}

struct PreparedReviewExtensions {
    extensions: Vec<LoadedExtension>,
    notifications: ExtensionNotificationHub,
    pending_trust_repo_root: Option<PathBuf>,
    configured_notices: Vec<StartupNotice>,
    application_notices: Vec<StartupNotice>,
    unknown_vcs_notices: Vec<StartupNotice>,
    load_notices: Vec<StartupNotice>,
    vcs_catalog: Option<VcsCatalog>,
}

struct PreloadedExtensionBootstrap {
    load: ExtensionLoadResult,
    discovery_catalog: VcsCatalog,
    configured_notices: Vec<StartupNotice>,
}

/// Retain provisional load ownership until bootstrap either commits it to the TUI or fails.
///
/// `LoadedExtension` is cloneable because VCS callbacks retain process handles. A plain dropped
/// vector therefore cannot guarantee immediate revocation when a provisional catalog is still in
/// scope. This guard explicitly closes the owning load result on every error path.
struct ProvisionalReviewExtensionLoad(Option<ExtensionLoadResult>);

impl ProvisionalReviewExtensionLoad {
    fn new(result: ExtensionLoadResult) -> Self {
        Self(Some(result))
    }

    fn result(&self) -> &ExtensionLoadResult {
        self.0
            .as_ref()
            .expect("provisional extension load is still owned")
    }

    fn take(&mut self) -> ExtensionLoadResult {
        self.0
            .take()
            .expect("provisional extension load is still owned")
    }
}

impl Drop for ProvisionalReviewExtensionLoad {
    fn drop(&mut self) {
        if let Some(result) = &mut self.0 {
            result.retire();
        }
    }
}

type WorkdeckAppBootstrap = AppBootstrap<PreparedReviewExtensions, VcsCatalog>;

struct SelectedVcsAdapter {
    adapter: VcsAdapter,
    unknown_id_notice: Option<StartupNotice>,
}

struct LoadedVcsChangeset {
    changeset: Changeset,
    repo_root: PathBuf,
}

struct LoadedReviewChangeset {
    changeset: Changeset,
    repo_root: Option<PathBuf>,
}

fn prepare_review_extensions(
    cwd: &Path,
    review: &mut ReviewCliOptions,
    raw_review: &ReviewCliOptions,
    previous_load: Option<ExtensionLoadResult>,
    discovery_catalog: Option<&VcsCatalog>,
) -> Result<PreparedReviewExtensions> {
    load_review_extensions(cwd, review, raw_review, previous_load, discovery_catalog)
}

fn load_extensions_before_changeset<E, C>(
    load_extensions: impl FnOnce() -> Result<E>,
    load_changeset: impl FnOnce() -> Result<C>,
) -> Result<(E, C)> {
    let extensions = load_extensions()?;
    let changeset = load_changeset()?;
    Ok((extensions, changeset))
}

fn compose_review_vcs_catalog(extensions: &[LoadedExtension]) -> VcsCatalog {
    let base = bundled_vcs_catalog();
    let resolution = resolve_loaded_extension_registrations(extensions, base);
    extend_vcs_catalog(base, resolved_native_vcs_adapters(extensions, &resolution))
}

fn select_review_vcs_adapter(
    cwd: &Path,
    configured_id: Option<&str>,
    catalog: &VcsCatalog,
) -> Result<SelectedVcsAdapter> {
    if let Some(id) = configured_id {
        if let Ok(adapter) = get_vcs_adapter(id, catalog) {
            return Ok(SelectedVcsAdapter {
                adapter: adapter.clone(),
                unknown_id_notice: None,
            });
        }
        let adapter = detect_vcs(cwd, catalog)
            .and_then(|detection| get_vcs_adapter(&detection.id, catalog).ok())
            .map_or_else(|| get_default_vcs_adapter(catalog), Ok)?
            .clone();
        let message = sanitize_terminal_line(&format!(
            "Unknown vcs \"{id}\" • falling back to {}. Install an extension that registers it, or fix the id in Workdeck config.",
            adapter.id
        ));
        return Ok(SelectedVcsAdapter {
            adapter,
            unknown_id_notice: Some(StartupNotice::new(format!("vcs:unknown:{id}"), message)),
        });
    }
    if let Some(detection) = detect_vcs(cwd, catalog) {
        return Ok(SelectedVcsAdapter {
            adapter: get_vcs_adapter(&detection.id, catalog)?.clone(),
            unknown_id_notice: None,
        });
    }
    Ok(SelectedVcsAdapter {
        adapter: get_default_vcs_adapter(catalog)?.clone(),
        unknown_id_notice: None,
    })
}

fn vcs_input_options_mut(input: &mut VcsReviewInput) -> &mut CommonOptions {
    match input {
        VcsReviewInput::Diff(input) => &mut input.options,
        VcsReviewInput::Show(input) => &mut input.options,
        VcsReviewInput::StashShow(input) => &mut input.options,
    }
}

fn load_selected_vcs_changeset(
    cwd: &Path,
    adapter: &VcsAdapter,
    catalog: &VcsCatalog,
    input: &VcsReviewInput,
) -> Result<LoadedVcsChangeset> {
    let operation = operation_from_input(input.clone());
    let result = load_vcs_review(
        adapter,
        &operation,
        &VcsLoadContext {
            cwd: cwd.to_owned(),
        },
        catalog,
    )?;
    let (suffix, source) = match input {
        VcsReviewInput::Diff(input) => {
            let source = if let Some(endpoints) = &input.range_endpoints {
                ChangesetSource::Revision {
                    from: Some(endpoints.from.clone()),
                    to: endpoints.to.clone(),
                }
            } else if let Some(range) = &input.range {
                ChangesetSource::Revision {
                    from: Some(range.clone()),
                    to: "WORKTREE".into(),
                }
            } else {
                ChangesetSource::WorkingTree {
                    staged: input.staged,
                }
            };
            ("working".to_owned(), source)
        }
        VcsReviewInput::Show(input) => {
            let reference = input.reference.as_deref().unwrap_or("HEAD");
            (
                format!("show:{reference}"),
                ChangesetSource::Revision {
                    from: None,
                    to: reference.into(),
                },
            )
        }
        VcsReviewInput::StashShow(input) => {
            let reference = input.reference.as_deref().unwrap_or("stash@{0}");
            (
                format!("stash:{reference}"),
                ChangesetSource::Stash {
                    reference: reference.into(),
                },
            )
        }
    };
    let repo_root = result.repo_root.clone();
    let changeset =
        materialize_vcs_patch_result(result, format!("{}:{suffix}", adapter.id), source)
            .map_err(anyhow::Error::from)?;
    Ok(LoadedVcsChangeset {
        changeset,
        repo_root,
    })
}

fn prepare_piped_review_input(command: Option<&Command>) -> Result<Option<PreparedPipedInput>> {
    let reads_stdin = match command {
        Some(Command::Pager { .. }) => true,
        Some(Command::Patch { file, .. }) => {
            file.as_deref().is_none_or(|path| path == Path::new("-"))
        }
        _ => false,
    };
    if std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut text = String::new();
    if reads_stdin {
        std::io::stdin()
            .read_to_string(&mut text)
            .context("failed to read piped review input")?;
    }
    let terminal =
        should_open_controlling_terminal(command, &text, false, std::io::stdout().is_terminal())
            .then(attach_controlling_terminal_input)
            .transpose()?;
    if !reads_stdin && terminal.is_none() {
        return Ok(None);
    }
    Ok(Some(PreparedPipedInput { text, terminal }))
}

fn should_open_controlling_terminal(
    command: Option<&Command>,
    piped_text: &str,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    if stdin_is_terminal || !stdout_is_terminal {
        return false;
    }
    match command {
        None
        | Some(
            Command::Diff { .. }
            | Command::Show { .. }
            | Command::Stash { .. }
            | Command::Patch { .. }
            | Command::Difftool { .. },
        ) => true,
        Some(Command::Pager { .. }) => workdeck_cli::pager::looks_like_patch_input(piped_text),
        _ => false,
    }
}

fn handle_review_command(
    cwd: &Path,
    command: Command,
    raw_review: ReviewCliOptions,
    mut preloaded_extensions: Option<ExtensionLoadResult>,
    discovery_catalog: Option<VcsCatalog>,
    mut prepared_piped_input: Option<PreparedPipedInput>,
) -> Result<()> {
    match command {
        Command::Diff {
            revisions,
            files,
            staged,
            exclude_untracked,
            include_untracked,
            pathspec,
            mut review,
        } => {
            if validate_diff_file_arguments(&files, &revisions, staged, &pathspec)? {
                let left = files[0].clone();
                let right = files[1].clone();
                let (prepared_extensions, changeset) = load_extensions_before_changeset(
                    || {
                        prepare_review_extensions(
                            cwd,
                            &mut review,
                            &raw_review,
                            preloaded_extensions.take(),
                            discovery_catalog.as_ref(),
                        )
                    },
                    || load_file_comparison(cwd, &left, &right).map_err(anyhow::Error::from),
                )?;
                let input = CliInput::Files(FileCommandInput {
                    left: left.to_string_lossy().into_owned(),
                    right: right.to_string_lossy().into_owned(),
                    options: review.common_options(),
                });
                let initial_watch_signature =
                    capture_initial_watch_signature(&review, &input, cwd, None);
                let mut reload =
                    || load_file_comparison(cwd, &left, &right).map_err(anyhow::Error::from);
                return run_review_with_preloaded_extensions(
                    cwd,
                    LoadedReviewChangeset {
                        changeset,
                        repo_root: None,
                    },
                    review,
                    input,
                    initial_watch_signature,
                    Some(&mut reload),
                    prepared_extensions,
                );
            }
            let (from, target) = match revisions.as_slice() {
                [] => (None, None),
                [target] => (None, Some(target.clone())),
                [from, to] => (Some(from.clone()), Some(to.clone())),
                _ => unreachable!("clap limits revisions to two"),
            };
            let mut prepared_extensions = prepare_review_extensions(
                cwd,
                &mut review,
                &raw_review,
                preloaded_extensions.take(),
                discovery_catalog.as_ref(),
            )?;
            let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
            prepared_extensions.vcs_catalog = Some(catalog.clone());
            let mut input_options = review.common_options();
            input_options.exclude_untracked =
                Some(!include_untracked && (exclude_untracked || review.config_exclude_untracked));
            let mut vcs_input = VcsReviewInput::Diff(VcsDiffCommandInput {
                range: from.is_none().then(|| target.clone()).flatten(),
                range_endpoints: from
                    .clone()
                    .zip(target.clone())
                    .map(|(from, to)| VcsRangeEndpoints { from, to }),
                staged,
                pathspecs: pathspec,
                options: input_options,
            });
            let selection = select_review_vcs_adapter(cwd, review.configured_vcs_id(), &catalog)?;
            if let Some(notice) = selection.unknown_id_notice {
                prepared_extensions.unknown_vcs_notices.push(notice);
            }
            let adapter = selection.adapter;
            vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
            let session_input = match &vcs_input {
                VcsReviewInput::Diff(input) => CliInput::Vcs(input.clone()),
                _ => unreachable!(),
            };
            let initial_watch_signature =
                capture_initial_watch_signature(&review, &session_input, cwd, Some(&catalog));
            let loaded = load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)?;
            let mut reload = || {
                load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)
                    .map(|loaded| loaded.changeset)
            };
            run_review_with_preloaded_extensions(
                cwd,
                LoadedReviewChangeset {
                    changeset: loaded.changeset,
                    repo_root: Some(loaded.repo_root),
                },
                review,
                session_input,
                initial_watch_signature,
                Some(&mut reload),
                prepared_extensions,
            )
        }
        Command::Show {
            target,
            pathspec,
            mut review,
        } => {
            let mut prepared_extensions = prepare_review_extensions(
                cwd,
                &mut review,
                &raw_review,
                preloaded_extensions.take(),
                discovery_catalog.as_ref(),
            )?;
            let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
            prepared_extensions.vcs_catalog = Some(catalog.clone());
            let mut vcs_input = VcsReviewInput::Show(VcsShowCommandInput {
                reference: target.clone(),
                pathspecs: pathspec.clone(),
                options: review.common_options(),
            });
            let selection = select_review_vcs_adapter(cwd, review.configured_vcs_id(), &catalog)?;
            if let Some(notice) = selection.unknown_id_notice {
                prepared_extensions.unknown_vcs_notices.push(notice);
            }
            let adapter = selection.adapter;
            vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
            let session_input = match &vcs_input {
                VcsReviewInput::Show(input) => CliInput::Show(input.clone()),
                _ => unreachable!(),
            };
            let initial_watch_signature =
                capture_initial_watch_signature(&review, &session_input, cwd, Some(&catalog));
            let loaded = load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)?;
            let mut reload = || {
                load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)
                    .map(|loaded| loaded.changeset)
            };
            run_review_with_preloaded_extensions(
                cwd,
                LoadedReviewChangeset {
                    changeset: loaded.changeset,
                    repo_root: Some(loaded.repo_root),
                },
                review,
                session_input,
                initial_watch_signature,
                Some(&mut reload),
                prepared_extensions,
            )
        }
        Command::Stash {
            command:
                StashCommand::Show {
                    reference,
                    mut review,
                },
        } => {
            let mut prepared_extensions = prepare_review_extensions(
                cwd,
                &mut review,
                &raw_review,
                preloaded_extensions.take(),
                discovery_catalog.as_ref(),
            )?;
            let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
            prepared_extensions.vcs_catalog = Some(catalog.clone());
            let configured_id = review.configured_vcs_id().or(Some("git"));
            let mut vcs_input = VcsReviewInput::StashShow(VcsStashShowCommandInput {
                reference: reference.clone(),
                options: review.common_options(),
            });
            let selection = select_review_vcs_adapter(cwd, configured_id, &catalog)?;
            if let Some(notice) = selection.unknown_id_notice {
                prepared_extensions.unknown_vcs_notices.push(notice);
            }
            let adapter = selection.adapter;
            vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
            let session_input = match &vcs_input {
                VcsReviewInput::StashShow(input) => CliInput::StashShow(input.clone()),
                _ => unreachable!(),
            };
            let initial_watch_signature =
                capture_initial_watch_signature(&review, &session_input, cwd, Some(&catalog));
            let loaded = load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)?;
            let mut reload = || {
                load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)
                    .map(|loaded| loaded.changeset)
            };
            run_review_with_preloaded_extensions(
                cwd,
                LoadedReviewChangeset {
                    changeset: loaded.changeset,
                    repo_root: Some(loaded.repo_root),
                },
                review,
                session_input,
                initial_watch_signature,
                Some(&mut reload),
                prepared_extensions,
            )
        }
        Command::Patch { file, mut review } => {
            let reload_path = file.clone().filter(|path| path != Path::new("-"));
            let (prepared_extensions, (patch, label)) = load_extensions_before_changeset(
                || {
                    prepare_review_extensions(
                        cwd,
                        &mut review,
                        &raw_review,
                        preloaded_extensions.take(),
                        discovery_catalog.as_ref(),
                    )
                },
                || match file {
                    Some(path) if path != Path::new("-") => {
                        let patch = std::fs::read_to_string(&path)
                            .with_context(|| format!("failed to read patch {}", path.display()))?;
                        Ok((patch, path.display().to_string()))
                    }
                    _ => {
                        let patch = if let Some(prepared) = prepared_piped_input.as_mut() {
                            std::mem::take(&mut prepared.text)
                        } else {
                            let mut patch = String::new();
                            std::io::stdin()
                                .read_to_string(&mut patch)
                                .context("failed to read patch from stdin")?;
                            patch
                        };
                        Ok((patch, "stdin patch".to_owned()))
                    }
                },
            )?;
            let reload_input = reload_path.as_ref().map(|path| {
                CliInput::Patch(PatchCommandInput {
                    file: Some(path.to_string_lossy().into_owned()),
                    text: None,
                    options: review.common_options(),
                })
            });
            let initial_watch_signature = reload_input
                .as_ref()
                .and_then(|input| capture_initial_watch_signature(&review, input, cwd, None));
            let session_input = reload_input.clone().unwrap_or_else(|| {
                CliInput::Patch(PatchCommandInput {
                    file: None,
                    text: Some(patch.clone()),
                    options: review.common_options(),
                })
            });
            let changeset = parse_patch_input(&patch, label).map_err(anyhow::Error::from)?;
            if let Some(path) = reload_path {
                let mut reload = || {
                    let patch = std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read patch {}", path.display()))?;
                    parse_patch_input(&patch, path.display().to_string())
                        .map_err(anyhow::Error::from)
                };
                run_review_with_preloaded_extensions(
                    cwd,
                    LoadedReviewChangeset {
                        changeset,
                        repo_root: None,
                    },
                    review,
                    session_input,
                    initial_watch_signature,
                    Some(&mut reload),
                    prepared_extensions,
                )
            } else {
                run_review_with_preloaded_extensions(
                    cwd,
                    LoadedReviewChangeset {
                        changeset,
                        repo_root: None,
                    },
                    review,
                    session_input,
                    None,
                    None,
                    prepared_extensions,
                )
            }
        }
        Command::Difftool {
            left,
            right,
            path,
            mut review,
        } => {
            let (prepared_extensions, changeset) = load_extensions_before_changeset(
                || {
                    prepare_review_extensions(
                        cwd,
                        &mut review,
                        &raw_review,
                        preloaded_extensions.take(),
                        discovery_catalog.as_ref(),
                    )
                },
                || {
                    load_difftool_comparison(cwd, &left, &right, path.as_deref())
                        .map_err(anyhow::Error::from)
                },
            )?;
            let input = CliInput::DiffTool(DiffToolCommandInput {
                left: left.to_string_lossy().into_owned(),
                right: right.to_string_lossy().into_owned(),
                path: path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                options: review.common_options(),
            });
            let initial_watch_signature =
                capture_initial_watch_signature(&review, &input, cwd, None);
            let display_path = path.clone();
            let mut reload = || {
                load_difftool_comparison(cwd, &left, &right, display_path.as_deref())
                    .map_err(anyhow::Error::from)
            };
            run_review_with_preloaded_extensions(
                cwd,
                LoadedReviewChangeset {
                    changeset,
                    repo_root: None,
                },
                review,
                input,
                initial_watch_signature,
                Some(&mut reload),
                prepared_extensions,
            )
        }
        Command::Pager { mut review } => {
            let input = if let Some(prepared) = prepared_piped_input.as_mut() {
                std::mem::take(&mut prepared.text)
            } else {
                let mut input = String::new();
                std::io::stdin()
                    .read_to_string(&mut input)
                    .context("failed to read pager input")?;
                input
            };
            if workdeck_cli::pager::looks_like_patch_input(&input) {
                let (prepared_extensions, changeset) = load_extensions_before_changeset(
                    || {
                        prepare_review_extensions(
                            cwd,
                            &mut review,
                            &raw_review,
                            preloaded_extensions.take(),
                            discovery_catalog.as_ref(),
                        )
                    },
                    || parse_patch_input(&input, "pager").map_err(anyhow::Error::from),
                )?;
                let session_input = CliInput::Patch(PatchCommandInput {
                    file: None,
                    text: Some(input),
                    options: review.common_options(),
                });
                run_review_with_preloaded_extensions(
                    cwd,
                    LoadedReviewChangeset {
                        changeset,
                        repo_root: None,
                    },
                    review,
                    session_input,
                    None,
                    None,
                    prepared_extensions,
                )
            } else {
                let context = workdeck_cli::pager::PlainTextPagerContext::current();
                workdeck_cli::pager::page_plain_text(&input, &context).map_err(anyhow::Error::from)
            }
        }
        _ => unreachable!("non-review command passed to review handler"),
    }
}

fn validate_diff_file_arguments(
    files: &[PathBuf],
    revisions: &[String],
    staged: bool,
    pathspecs: &[String],
) -> Result<bool> {
    if files.is_empty() {
        return Ok(false);
    }
    if files.len() != 2 || !revisions.is_empty() || staged || !pathspecs.is_empty() {
        bail!(
            "Use `workdeck diff --files <left> <right>` with exactly two file paths and no revision, staged, or pathspec arguments."
        );
    }
    Ok(true)
}

fn attach_controlling_terminal_input() -> Result<workdeck_tui::ControllingTerminal<File>> {
    let terminal = workdeck_tui::open_controlling_terminal().context(
        "piped interactive review requires an attached controlling terminal for keyboard input",
    )?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::Console::{STD_INPUT_HANDLE, SetStdHandle};

        // SAFETY: the File owns a valid CONIN$ handle and remains alive in the returned guard until
        // the interactive review has completed.
        let attached =
            unsafe { SetStdHandle(STD_INPUT_HANDLE, terminal.input.as_raw_handle() as HANDLE) };
        if attached == 0 {
            return Err(std::io::Error::last_os_error())
                .context("attach Windows console to piped review stdin");
        }
    }
    Ok(terminal)
}

fn probe_initial_theme_mode<I: ThemeProbeInput, W: Write>(
    theme: Option<&str>,
    stdout_is_terminal: bool,
    input: Option<&mut I>,
    output: &mut W,
    timeout: Duration,
) -> std::io::Result<Option<TerminalThemeMode>> {
    if !stdout_is_terminal || theme.is_some_and(|theme| theme != "auto") {
        return Ok(None);
    }
    let Some(input) = input else {
        return Ok(None);
    };
    workdeck_tui::detect_terminal_theme_mode_from_background(input, output, timeout)
}

fn detect_initial_review_theme_mode(
    review: &ReviewCliOptions,
    terminal: Option<&mut workdeck_tui::ControllingTerminal<File>>,
) -> Result<Option<TerminalThemeMode>> {
    let stdout_is_terminal = std::io::stdout().is_terminal();
    if !stdout_is_terminal || review.theme.as_deref().is_some_and(|theme| theme != "auto") {
        return Ok(None);
    }
    let mut output = std::io::stdout();
    if let Some(terminal) = terminal {
        return probe_initial_theme_mode(
            review.theme.as_deref(),
            true,
            Some(&mut terminal.input),
            &mut output,
            workdeck_tui::DEFAULT_THEME_PROBE_TIMEOUT,
        )
        .context("detect terminal background for automatic theme selection");
    }
    if std::io::stdin().is_terminal() {
        let mut input = std::io::stdin();
        return probe_initial_theme_mode(
            review.theme.as_deref(),
            true,
            Some(&mut input),
            &mut output,
            workdeck_tui::DEFAULT_THEME_PROBE_TIMEOUT,
        )
        .context("detect terminal background for automatic theme selection");
    }
    Ok(None)
}

fn run_review_with_preloaded_extensions(
    cwd: &Path,
    loaded: LoadedReviewChangeset,
    review: ReviewCliOptions,
    input: CliInput,
    initial_watch_signature: Option<String>,
    reloader: Option<&mut dyn FnMut() -> Result<Changeset>>,
    prepared_extensions: PreparedReviewExtensions,
) -> Result<()> {
    if review.watch && !review.no_watch && reloader.is_none() {
        bail!("--watch requires a file- or VCS-backed review input");
    }
    let bootstrap = prepare_app_bootstrap(
        cwd,
        loaded.repo_root,
        loaded.changeset,
        &review,
        input,
        initial_watch_signature,
        prepared_extensions,
    )?;
    run_app_bootstrap(bootstrap, review, reloader)
}

fn prepare_app_bootstrap(
    cwd: &Path,
    repo_root: Option<PathBuf>,
    mut changeset: Changeset,
    review: &ReviewCliOptions,
    input: CliInput,
    initial_watch_signature: Option<String>,
    mut prepared_extensions: PreparedReviewExtensions,
) -> Result<WorkdeckAppBootstrap> {
    apply_agent_context(cwd, review.agent_context.as_deref(), &mut changeset)?;
    changeset = apply_review_extensions(changeset, &mut prepared_extensions.extensions)?;
    Ok(build_app_bootstrap(
        cwd,
        repo_root,
        changeset,
        review,
        input,
        initial_watch_signature,
        prepared_extensions,
    ))
}

fn build_app_bootstrap(
    cwd: &Path,
    repo_root: Option<PathBuf>,
    changeset: Changeset,
    review: &ReviewCliOptions,
    input: CliInput,
    initial_watch_signature: Option<String>,
    mut prepared_extensions: PreparedReviewExtensions,
) -> WorkdeckAppBootstrap {
    let options = input.options();
    let initial_mode = options.mode.unwrap_or_default();
    let initial_theme = options.theme.clone();
    let initial_show_line_numbers = options.line_numbers.unwrap_or(true);
    let initial_tab_width = options.tab_width.unwrap_or(4);
    let initial_file_gap = options.file_gap.unwrap_or(1);
    let initial_hunk_gap = options.hunk_gap.unwrap_or(0);
    let initial_wrap_lines = options.wrap_lines.unwrap_or(false);
    let initial_show_hunk_headers = options.hunk_headers.unwrap_or(true);
    let initial_show_menu_bar = options.menu_bar.unwrap_or(true);
    let initial_sidebar = options.sidebar.unwrap_or_default();
    let initial_show_agent_notes = options.agent_notes.unwrap_or(false);
    let initial_copy_decorations = options.copy_decorations.unwrap_or(false);
    let initial_cursor_line = options.cursor_line.unwrap_or_default();
    let vcs_catalog = prepared_extensions.vcs_catalog.take();
    let session_themes = collect_review_custom_themes(review, &prepared_extensions.extensions);
    let startup_notices =
        take_review_startup_notices(&mut prepared_extensions, session_themes.notices);

    AppBootstrap {
        input,
        reload_context: ReloadContext {
            cwd: cwd.to_owned(),
            repo_root,
            initial_watch_signature,
            vcs_catalog,
        },
        changeset,
        initial_mode,
        initial_theme,
        initial_theme_mode: review.initial_theme_mode,
        custom_themes: session_themes.themes,
        initial_show_line_numbers,
        initial_tab_width,
        initial_file_gap,
        initial_hunk_gap,
        initial_wrap_lines,
        initial_show_hunk_headers,
        initial_show_menu_bar,
        initial_sidebar,
        initial_show_agent_notes,
        initial_copy_decorations,
        initial_cursor_line,
        startup_notices,
        view_preferences_config_path: review.view_preferences_config_path.clone(),
        keybindings: review.keybindings.clone(),
        keybinding_notices: review.keybinding_notices.clone(),
        extensions: Some(prepared_extensions),
    }
}

fn take_review_startup_notices(
    prepared: &mut PreparedReviewExtensions,
    theme_notices: Vec<StartupNotice>,
) -> Vec<StartupNotice> {
    let mut notices = std::mem::take(&mut prepared.configured_notices);
    notices.extend(theme_notices);
    notices.append(&mut prepared.application_notices);
    notices.append(&mut prepared.unknown_vcs_notices);
    notices.append(&mut prepared.load_notices);
    notices
}

fn collect_review_custom_themes(
    review: &ReviewCliOptions,
    extensions: &[LoadedExtension],
) -> workdeck_core::SessionCustomThemes {
    let resolution = resolve_loaded_extension_registrations(extensions, bundled_vcs_catalog());
    let registered = extensions
        .iter()
        .enumerate()
        .flat_map(|(extension_index, extension)| {
            let resolution = &resolution;
            extension
                .handshake
                .registrations
                .iter()
                .enumerate()
                .filter_map(move |(registration_index, registration)| {
                    resolution
                        .accepts(extension_index, registration_index)
                        .then_some((extension, registration))
                })
        })
        .filter_map(|(extension, registration)| {
            let Registration::Theme(theme) = registration else {
                return None;
            };
            Some(registered_custom_theme(&extension.manifest.id, theme))
        })
        .collect::<Vec<_>>();
    collect_session_custom_themes(&review.custom_themes, &registered)
}

fn registered_custom_theme(
    extension_id: &str,
    theme: &workdeck_extension_api::ThemeRegistration,
) -> RegisteredCustomTheme {
    let mut value = serde_json::Map::new();
    value.insert("id".into(), Value::String(theme.id.clone()));
    if let Some(base) = &theme.base {
        value.insert("base".into(), Value::String(base.clone()));
    }
    for (key, color) in &theme.colors {
        value.insert(key.clone(), Value::String(color.clone()));
    }
    RegisteredCustomTheme {
        extension_id: extension_id.into(),
        theme: Value::Object(value),
    }
}

fn run_app_bootstrap(
    bootstrap: WorkdeckAppBootstrap,
    review: ReviewCliOptions,
    reloader: Option<&mut dyn FnMut() -> Result<Changeset>>,
) -> Result<()> {
    let AppBootstrap {
        input,
        reload_context,
        changeset,
        initial_mode,
        initial_theme,
        initial_theme_mode,
        custom_themes,
        initial_show_line_numbers,
        initial_tab_width,
        initial_file_gap,
        initial_hunk_gap,
        initial_wrap_lines,
        initial_show_hunk_headers,
        initial_show_menu_bar,
        initial_sidebar,
        initial_show_agent_notes,
        initial_copy_decorations,
        initial_cursor_line,
        startup_notices,
        keybindings,
        keybinding_notices,
        extensions,
        ..
    } = bootstrap;
    let cwd = reload_context.cwd;
    let initial_watch_signature = reload_context.initial_watch_signature;
    let vcs_catalog = reload_context.vcs_catalog;
    let prepared_extensions =
        extensions.ok_or_else(|| anyhow::anyhow!("review bootstrap has no extension state"))?;
    let PreparedReviewExtensions {
        extensions,
        notifications,
        pending_trust_repo_root,
        configured_notices: _,
        application_notices: _,
        unknown_vcs_notices: _,
        load_notices: _,
        vcs_catalog: _,
    } = prepared_extensions;
    let mut options = review.tui_options();
    options.layout = match initial_mode {
        InputLayoutMode::Auto => LayoutMode::Auto,
        InputLayoutMode::Split => LayoutMode::Split,
        InputLayoutMode::Stack => LayoutMode::Stack,
    };
    options.line_numbers = initial_show_line_numbers;
    options.tab_width = initial_tab_width;
    options.file_gap = initial_file_gap;
    options.hunk_gap = initial_hunk_gap;
    options.wrap_lines = initial_wrap_lines;
    options.hunk_headers = initial_show_hunk_headers;
    options.show_menu_bar = initial_show_menu_bar;
    options.sidebar = initial_sidebar != SidebarVisibility::Hidden;
    options.agent_notes = initial_show_agent_notes;
    options.copy_decorations = initial_copy_decorations;
    options.cursor_line = match initial_cursor_line {
        InputCursorLine::Row => CursorLineMode::Row,
        InputCursorLine::Number => CursorLineMode::Number,
        InputCursorLine::Off => CursorLineMode::Off,
    };
    let theme = workdeck_tui::resolve_theme(
        initial_theme.as_deref(),
        initial_theme_mode.map(Into::into),
        &custom_themes,
    );
    options.theme = if options.transparent_background {
        workdeck_tui::with_transparent_surfaces(&theme)
    } else {
        theme
    };
    options.keybindings = keybindings;
    options.keybinding_notices = keybinding_notices;
    options.review_input = Some(input.clone());
    options.extension_notifications = Some(notifications.clone());
    options.startup_notices = startup_notices;
    options.pending_extension_trust_repo_root = pending_trust_repo_root;
    options.extension_trust_handler = Some(review_extension_trust_handler(
        &cwd,
        &review,
        &notifications,
    ));
    options.command_cwd = Some(cwd.clone());
    options.repo = Some(resolve_review_repo_root(
        &cwd,
        review.preference(),
        reload_context.repo_root.as_deref(),
    ));
    if let Some(reloader) = reloader {
        let agent_context = review.agent_context.clone();
        let mut reload_extensions = extensions.clone();
        let mut decorated_reload = || {
            let mut changeset = reloader()?;
            apply_agent_context(&cwd, agent_context.as_deref(), &mut changeset)?;
            apply_review_extensions(changeset, &mut reload_extensions)
        };
        match vcs_catalog {
            None => workdeck_tui::run_review_with_extensions_input_reload_with_signature(
                changeset,
                options,
                extensions,
                workdeck_tui::ReviewWatchInput {
                    input,
                    cwd: cwd.clone(),
                    initial_signature: initial_watch_signature,
                },
                &mut decorated_reload,
            ),
            Some(catalog) => {
                workdeck_tui::run_review_with_extensions_catalog_input_reload_with_signature(
                    changeset,
                    options,
                    extensions,
                    workdeck_tui::ReviewWatchInput {
                        input,
                        cwd: cwd.clone(),
                        initial_signature: initial_watch_signature,
                    },
                    catalog,
                    &mut decorated_reload,
                )
            }
        }
    } else {
        workdeck_tui::run_review_with_extensions(changeset, options, extensions)
    }
}

/// Capture before sidecar or changeset I/O so mutations racing the initial load
/// are still visible when the watch controller mounts.
fn capture_initial_watch_signature(
    review: &ReviewCliOptions,
    input: &CliInput,
    cwd: &Path,
    vcs_catalog: Option<&VcsCatalog>,
) -> Option<String> {
    if !review.watch || review.no_watch {
        return None;
    }
    compute_watch_signature(input, WatchSignatureContext { cwd, vcs_catalog }).ok()
}

fn resolve_review_repo_root(
    cwd: &Path,
    preference: ProviderPreference,
    vcs_repo_root: Option<&Path>,
) -> PathBuf {
    vcs_repo_root.map_or_else(
        || {
            AnyProvider::discover(cwd, preference)
                .ok()
                .map_or_else(|| cwd.to_owned(), |provider| provider.root().to_owned())
        },
        Path::to_owned,
    )
}

fn apply_review_extensions(
    mut changeset: Changeset,
    extensions: &mut [LoadedExtension],
) -> Result<Changeset> {
    let language_registry = build_review_language_registry(extensions);
    for file in &mut changeset.files {
        let language = language_registry.language_for_path(&file.path);
        file.language = (language != "text").then_some(language);
    }
    for extension in extensions.iter_mut() {
        changeset = extension.apply_changeset_transforms(changeset);
    }
    changeset.refresh_review_identities();
    Ok(changeset)
}

/// Build a session-local selector set so an aborted bootstrap cannot leak registrations into the
/// active review. This is the native transaction boundary corresponding to Hunk's snapshot and
/// restore pair around `loadConfiguredSessionBootstrap`.
fn build_review_language_registry(extensions: &[LoadedExtension]) -> LanguageRegistry {
    let resolution = resolve_loaded_extension_registrations(extensions, bundled_vcs_catalog());
    let file_languages = extensions
        .iter()
        .enumerate()
        .flat_map(|(extension_index, extension)| {
            let resolution = &resolution;
            extension
                .handshake
                .registrations
                .iter()
                .enumerate()
                .filter_map(move |(registration_index, registration)| {
                    resolution
                        .accepts(extension_index, registration_index)
                        .then_some(registration)
                })
        })
        .filter_map(|registration| match registration {
            Registration::FileLanguage(registration) => Some(LanguageRegistration {
                matcher: match &registration.matcher {
                    FileLanguageMatcher::Extension { value } => {
                        LanguageMatcher::Extension(value.clone())
                    }
                    FileLanguageMatcher::Filename { value } => {
                        LanguageMatcher::Filename(value.clone())
                    }
                    FileLanguageMatcher::Glob { value, target } => LanguageMatcher::Glob {
                        value: value.clone(),
                        target_path: *target == FileLanguageGlobTarget::Path,
                    },
                },
                language: registration.language.clone(),
                reserved: false,
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut language_registry = LanguageRegistry::default();
    language_registry.replace_extensions(file_languages);
    language_registry
}

fn apply_agent_context(cwd: &Path, path: Option<&Path>, changeset: &mut Changeset) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let source = if path == Path::new("-") {
        let mut source = String::new();
        std::io::stdin()
            .read_to_string(&mut source)
            .context("failed to read agent context from stdin")?;
        source
    } else {
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            cwd.join(path)
        };
        std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read agent context {}", path.display()))?
    };
    AgentContext::from_json(&source)?.apply_to(changeset);
    Ok(())
}

fn load_review_extensions(
    cwd: &Path,
    review: &mut ReviewCliOptions,
    raw_review: &ReviewCliOptions,
    previous_load: Option<ExtensionLoadResult>,
    discovery_catalog: Option<&VcsCatalog>,
) -> Result<PreparedReviewExtensions> {
    let notifications = previous_load
        .as_ref()
        .map(|previous| previous.notifications.clone())
        .unwrap_or_default();
    load_review_extensions_with_notifications(
        cwd,
        review,
        raw_review,
        notifications,
        previous_load,
        discovery_catalog,
    )
}

fn load_review_extensions_with_notifications(
    cwd: &Path,
    review: &mut ReviewCliOptions,
    raw_review: &ReviewCliOptions,
    notifications: ExtensionNotificationHub,
    previous_load: Option<ExtensionLoadResult>,
    discovery_catalog: Option<&VcsCatalog>,
) -> Result<PreparedReviewExtensions> {
    load_review_extensions_with_config_loader(
        cwd,
        review,
        raw_review,
        notifications,
        previous_load,
        discovery_catalog,
        Config::load,
    )
}

fn load_review_extensions_with_config_loader(
    cwd: &Path,
    review: &mut ReviewCliOptions,
    raw_review: &ReviewCliOptions,
    notifications: ExtensionNotificationHub,
    previous_load: Option<ExtensionLoadResult>,
    discovery_catalog: Option<&VcsCatalog>,
    load_config: impl Fn(&Path) -> Result<Config>,
) -> Result<PreparedReviewExtensions> {
    let initial_repo = find_project_root_candidate_with_catalog(
        cwd,
        Some(discovery_catalog.unwrap_or_else(|| bundled_vcs_catalog())),
    );
    let first = load_review_extension_pass(
        cwd,
        review,
        notifications.clone(),
        initial_repo.as_deref(),
        previous_load,
        true,
    )?;
    let mut provisional = ProvisionalReviewExtensionLoad::new(first);
    let provisional_resolution = resolve_loaded_extension_registrations(
        &provisional.result().extensions,
        bundled_vcs_catalog(),
    );
    let provisional_adapters =
        resolved_native_vcs_adapters(&provisional.result().extensions, &provisional_resolution);
    let has_extension_vcs = !provisional_adapters.is_empty();
    let provisional_catalog = extend_vcs_catalog(bundled_vcs_catalog(), provisional_adapters);
    let extension_repo = find_project_root_candidate_with_catalog(cwd, Some(&provisional_catalog));

    if has_extension_vcs && extension_repo != initial_repo {
        let config = load_config(extension_repo.as_deref().unwrap_or(cwd))?;
        let initial_theme_mode = review.initial_theme_mode;
        let mut final_review = raw_review.clone();
        final_review.apply_config_defaults(&config);
        final_review.initial_theme_mode = initial_theme_mode;
        *review = final_review;

        let previous = provisional.take();
        let final_result = load_review_extension_pass(
            cwd,
            review,
            notifications.clone(),
            extension_repo.as_deref(),
            Some(previous),
            false,
        )?;
        provisional = ProvisionalReviewExtensionLoad::new(final_result);
    }

    let result = provisional.take();
    let _ = result.bind_event_bus();
    let resolution =
        resolve_loaded_extension_registrations(&result.extensions, bundled_vcs_catalog());
    Ok(PreparedReviewExtensions {
        extensions: result.extensions,
        notifications,
        pending_trust_repo_root: result.pending_trust_repo_root,
        configured_notices: review.startup_notices.clone(),
        application_notices: create_extension_apply_notices(&resolution.issues),
        unknown_vcs_notices: Vec::new(),
        load_notices: create_extension_load_notices(&result.issues),
        vcs_catalog: None,
    })
}

fn load_review_extension_pass(
    cwd: &Path,
    review: &ReviewCliOptions,
    notifications: ExtensionNotificationHub,
    repo_root: Option<&Path>,
    mut previous_load: Option<ExtensionLoadResult>,
    defer_event_bus_binding: bool,
) -> Result<ExtensionLoadResult> {
    if review.no_extensions {
        if let Some(previous) = &mut previous_load {
            previous.retire();
        }
        return Ok(create_empty_extension_load_result(cwd, notifications));
    }
    let config = user_config_root().map(|root| root.join("workdeck"));
    let trust = load_extension_trust_store();
    let global_extensions = config.as_ref().map(|config| config.join("extensions"));
    load_startup_extensions(LoadStartupExtensionsOptions {
        enabled: true,
        cwd,
        global_directory: global_extensions.as_deref(),
        repo_root,
        trust: &trust,
        explicit_paths: &review.extension,
        user_config_paths: &review.user_extension_paths,
        repo_config_paths: &review.repo_extension_paths,
        host_version: env!("CARGO_PKG_VERSION"),
        extension_configs: &review.extension_config,
        notifications: Some(notifications),
        previous_load,
        defer_event_bus_binding,
    })
    .map_err(anyhow::Error::from)
}

fn review_extension_trust_handler(
    cwd: &Path,
    review: &ReviewCliOptions,
    notifications: &ExtensionNotificationHub,
) -> ExtensionTrustHandler {
    let cwd = cwd.to_owned();
    let review = review.clone();
    let notifications = notifications.clone();
    ExtensionTrustHandler::new(move |repo_root, decision, load_extensions| {
        let state_path = resolve_app_state_path().ok_or_else(|| {
            ExtensionTrustHostError::Write(ExtensionTrustWriteError::Message(
                "could not resolve the Workdeck app-state path".into(),
            ))
        })?;
        let mut trust = load_extension_trust_store();
        trust.grant(repo_root, decision);
        workdeck_store::update_app_state_record(&state_path, trust.app_state_patch()).map_err(
            |error| {
                ExtensionTrustHostError::Write(ExtensionTrustWriteError::Message(error.to_string()))
            },
        )?;
        if !load_extensions {
            return Ok(Vec::new());
        }
        let mut review = review.clone();
        let raw_review = review.clone();
        let mut prepared = load_review_extensions_with_notifications(
            &cwd,
            &mut review,
            &raw_review,
            notifications.clone(),
            None,
            None,
        )
        .map_err(|_| ExtensionTrustHostError::Reload)?;
        if prepared.pending_trust_repo_root.is_some() {
            return Err(ExtensionTrustHostError::Reload);
        }
        for notice in take_review_startup_notices(&mut prepared, Vec::new()) {
            notifications.notify(notice.message, ExtensionNotifyType::Warning);
        }
        Ok(prepared.extensions)
    })
}

fn load_cli_extensions(
    cwd: &Path,
    explicit: &[PathBuf],
    disabled: bool,
) -> Result<PreloadedExtensionBootstrap> {
    let initial_repo = find_project_root_candidate_with_catalog(cwd, Some(bundled_vcs_catalog()));
    let mut configured = Config::load(initial_repo.as_deref().unwrap_or(cwd))?;
    let notifications = ExtensionNotificationHub::new();
    let first = load_cli_extension_pass(
        cwd,
        explicit,
        disabled,
        &configured,
        initial_repo.as_deref(),
        notifications.clone(),
        None,
    )?;
    let mut provisional = ProvisionalReviewExtensionLoad::new(first);
    let resolution = resolve_loaded_extension_registrations(
        &provisional.result().extensions,
        bundled_vcs_catalog(),
    );
    let adapters = resolved_native_vcs_adapters(&provisional.result().extensions, &resolution);
    let has_extension_vcs = !adapters.is_empty();
    let discovery_catalog = extend_vcs_catalog(bundled_vcs_catalog(), adapters);
    let extension_repo = find_project_root_candidate_with_catalog(cwd, Some(&discovery_catalog));
    if has_extension_vcs && extension_repo != initial_repo {
        configured = Config::load(extension_repo.as_deref().unwrap_or(cwd))?;
        let previous = provisional.take();
        let final_result = load_cli_extension_pass(
            cwd,
            explicit,
            disabled,
            &configured,
            extension_repo.as_deref(),
            notifications,
            Some(previous),
        )?;
        provisional = ProvisionalReviewExtensionLoad::new(final_result);
    }
    let result = provisional.take();
    Ok(PreloadedExtensionBootstrap {
        load: result,
        discovery_catalog,
        configured_notices: configured.startup_notices,
    })
}

fn load_cli_extension_pass(
    cwd: &Path,
    explicit: &[PathBuf],
    disabled: bool,
    configured: &Config,
    repo_root: Option<&Path>,
    notifications: ExtensionNotificationHub,
    mut previous_load: Option<ExtensionLoadResult>,
) -> Result<ExtensionLoadResult> {
    if disabled || !configured.resolved_extensions.enabled {
        if let Some(previous) = &mut previous_load {
            previous.retire();
        }
        return Ok(create_empty_extension_load_result(cwd, notifications));
    }
    let config_root = user_config_root().map(|root| root.join("workdeck"));
    let trust = load_extension_trust_store();
    let global_extensions = config_root.as_ref().map(|config| config.join("extensions"));
    load_startup_extensions(LoadStartupExtensionsOptions {
        enabled: true,
        cwd,
        global_directory: global_extensions.as_deref(),
        repo_root,
        trust: &trust,
        explicit_paths: explicit,
        user_config_paths: &configured.resolved_extensions.paths,
        repo_config_paths: &configured.resolved_extensions.repo_paths,
        host_version: env!("CARGO_PKG_VERSION"),
        extension_configs: configured.extension_configs(),
        notifications: Some(notifications),
        previous_load,
        defer_event_bus_binding: true,
    })
    .map_err(anyhow::Error::from)
}

fn registered_extension_cli_commands(
    extensions: &[LoadedExtension],
) -> Vec<RegisteredExtensionCliCommand> {
    extensions
        .iter()
        .enumerate()
        .flat_map(|(extension_index, extension)| {
            extension
                .handshake
                .registrations
                .iter()
                .filter_map(move |registration| {
                    let Registration::CliCommand(command) = registration else {
                        return None;
                    };
                    Some(RegisteredExtensionCliCommand {
                        extension_index,
                        extension_id: extension.manifest.id.clone(),
                        source_path: extension.manifest_path.clone(),
                        origin: extension.origin.label().into(),
                        command: extension_cli_commands::copy_extension_cli_command(command),
                    })
                })
        })
        .collect()
}

fn parse_delegated_args(
    cwd: &Path,
    extension_paths: &[PathBuf],
    extensions_disabled: bool,
    argv: Vec<String>,
) -> Result<Option<Args>> {
    let mut delegated = Vec::<std::ffi::OsString>::new();
    delegated.push("workdeck".into());
    delegated.push("--cwd".into());
    delegated.push(cwd.as_os_str().to_owned());
    for path in extension_paths {
        delegated.push("--extension".into());
        delegated.push(path.as_os_str().to_owned());
    }
    if extensions_disabled {
        delegated.push("--no-extensions".into());
    }
    delegated.extend(argv.into_iter().map(Into::into));
    match Args::try_parse_from(delegated) {
        Ok(args) => Ok(Some(args)),
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            if exit_code == 0 {
                Ok(None)
            } else {
                Err(CommandExit(exit_code).into())
            }
        }
    }
}

fn handle_extension_cli_command(
    cwd: &Path,
    extension_paths: &[PathBuf],
    extensions_disabled: bool,
    command_name: &str,
    command_args: &[String],
) -> Result<()> {
    let mut extension_bootstrap = load_cli_extensions(cwd, extension_paths, extensions_disabled)?;
    let registered = registered_extension_cli_commands(&extension_bootstrap.load.extensions);
    let resolved = resolve_extension_cli_commands(&registered);
    let collision_issues = create_extension_cli_collision_issues(&registered, &resolved.collisions);

    let Some(extension_index) =
        find_extension_cli_command(command_name, &resolved).map(|owner| owner.extension_index)
    else {
        let mut message = format!("Unknown command: {command_name}");
        let descriptions = describe_extension_cli_commands(&resolved);
        if !descriptions.is_empty() {
            message.push_str("\nExtension commands available here:");
            for description in descriptions {
                message.push('\n');
                message.push_str(&description);
            }
        }
        if let Some(repo_root) = &extension_bootstrap.load.pending_trust_repo_root {
            message.push_str(&format!(
                "\nOpen a normal review in {} to decide whether to trust its extensions, then retry.",
                repo_root.display()
            ));
        }
        if !extension_bootstrap.load.issues.is_empty() {
            message.push_str(
                "\nOne or more extensions failed to load; rerun with WORKDECK_DEBUG=1 or open a review to inspect startup notices.",
            );
        }
        extension_bootstrap.load.retire();
        bail!(message);
    };

    for notice in &extension_bootstrap.configured_notices {
        eprintln!("workdeck: warning: {}", notice.message);
    }
    for issue in collision_issues {
        eprintln!("workdeck: warning: {}", issue.message);
    }
    let _ = extension_bootstrap.load.bind_event_bus();

    let command_cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    let signal_lease = Arc::new(ExtensionCliSignalLease::new());
    ctrlc::set_handler({
        let signal_lease = Arc::clone(&signal_lease);
        move || {
            if signal_lease.interrupt() == ExtensionCliInterruptAction::Exit {
                std::process::exit(130);
            }
        }
    })
    .context("failed to install extension CLI cancellation handler")?;
    let execution = {
        let mut stdin = std::io::stdin().lock();
        let mut stdout = std::io::stdout().lock();
        let mut stderr = std::io::stderr().lock();
        extension_bootstrap.load.extensions[extension_index]
            .invoke_cli_command_cancellable_with_input(
                command_name,
                command_args.to_vec(),
                &command_cwd,
                std::time::Duration::from_millis(
                    workdeck_extension_api::DEFAULT_CLI_REQUEST_TIMEOUT_MS,
                ),
                signal_lease.cancellation_flag(),
                &mut stdin,
                &mut stdout,
                &mut stderr,
            )
    };
    signal_lease.retire();
    let execution = execution?;

    match execution.result {
        CliCommandResult::Exit { code: 0 } => Ok(()),
        CliCommandResult::Exit { code } => Err(CommandExit(i32::from(code)).into()),
        CliCommandResult::Delegate { argv } => {
            let Some(delegated) =
                parse_delegated_args(cwd, extension_paths, extensions_disabled, argv)?
            else {
                return Ok(());
            };
            if matches!(delegated.command, Some(Command::External(_))) {
                bail!("Extension CLI commands may delegate only to built-in Workdeck commands.");
            }
            // Transfer complete ownership into built-in startup. The configured resolver can
            // extend this exact prefix without executing an unchanged extension factory twice.
            run_with_preloaded_extensions(delegated, Some(extension_bootstrap))
        }
    }
}

fn handle_global_command(cwd: &Path, command: Command) -> Result<()> {
    match command {
        Command::Session { command } => handle_live_session_command(command),
        Command::Extension { command } => handle_extension_command(cwd, command),
        Command::Migrate { command } => handle_migrate_command(cwd, command),
        Command::Markup { command } => handle_markup_command(command),
        Command::Skill { command } => handle_skill_command(command),
        Command::Update {
            version,
            method,
            check,
        } => handle_update_command(version, method, check),
        _ => unreachable!("non-global command passed to global handler"),
    }
}

fn handle_update_command(
    version: Option<String>,
    method: Option<String>,
    check: bool,
) -> Result<()> {
    let version = version
        .as_deref()
        .map(workdeck_cli::update::parse_update_version)
        .transpose()
        .map_err(format_update_error)?;
    let method = method
        .as_deref()
        .map(workdeck_cli::update::parse_update_method)
        .transpose()
        .map_err(format_update_error)?;
    let context =
        workdeck_cli::update::SelfUpdateContext::current().map_err(format_update_error)?;
    let result = workdeck_cli::update::run_self_update(
        &SelfUpdateCommandInput {
            version,
            method,
            check,
        },
        &context,
    )
    .map_err(format_update_error)?;
    if result.exit_code != 0 {
        return Err(CommandExit(result.exit_code).into());
    }
    Ok(())
}

fn format_update_error(error: workdeck_cli::update::UpdateError) -> anyhow::Error {
    let mut message = error.message;
    for suggestion in error.suggestions {
        message.push('\n');
        message.push_str(&suggestion);
    }
    anyhow::anyhow!(message)
}

fn handle_markup_command(command: MarkupCommand) -> Result<()> {
    match command {
        MarkupCommand::Guide => {
            print!("{}", workdeck_markup::GUIDE);
            Ok(())
        }
        MarkupCommand::Render {
            file,
            width,
            color,
            theme,
            json,
        } => {
            if width == 0 || width > 4096 {
                bail!("--width must be between 1 and 4096");
            }
            let mut source = String::new();
            if file == Path::new("-") {
                std::io::stdin()
                    .read_to_string(&mut source)
                    .context("failed to read STML from stdin")?;
            } else {
                source = std::fs::read_to_string(&file)
                    .with_context(|| format!("failed to read STML {}", file.display()))?;
            }
            let use_color = matches!(color, MarkupColor::Always)
                || matches!(color, MarkupColor::Auto) && std::io::stdout().is_terminal() && !json;
            let rendered = if use_color {
                let theme = workdeck_tui::resolve_theme(
                    Some(theme.as_deref().unwrap_or("github-dark-default")),
                    None,
                    &[],
                );
                workdeck_markup::render_stml_to_ansi(
                    &source,
                    width,
                    &workdeck_markup::StmlThemeColors {
                        accent: theme.accent,
                        accent_muted: theme.accent_muted,
                        added_sign_color: theme.added_sign_color,
                        removed_sign_color: theme.removed_sign_color,
                        file_modified: theme.file_modified,
                        muted: theme.muted,
                        panel_alt: theme.panel_alt,
                        text: theme.text,
                        panel: theme.panel,
                        note_border: theme.note_border,
                        background: theme.background,
                    },
                )
            } else {
                workdeck_markup::render_stml_to_text(&source, width)
            };
            if json {
                #[derive(serde::Serialize)]
                struct MarkupRenderJson<'a> {
                    width: usize,
                    lines: &'a [String],
                    notes: &'a [String],
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(&MarkupRenderJson {
                        width,
                        lines: &rendered.lines,
                        notes: &rendered.errors,
                    })?
                );
                return Ok(());
            }
            println!("{}", rendered.lines.join("\n"));
            for note in &rendered.errors {
                eprintln!("note: {note}");
            }
            Ok(())
        }
    }
}

fn handle_skill_command(command: SkillCommand) -> Result<()> {
    let SkillCommand::Path { name, json } = command;
    let source = match name.as_str() {
        "workdeck-review" | "review" => include_str!("../../../skills/workdeck-review/SKILL.md"),
        "workdeck-extensions" | "extensions" => {
            include_str!("../../../skills/workdeck-extensions/SKILL.md")
        }
        "workdeck-release" | "release" => {
            include_str!("../../../skills/workdeck-release/SKILL.md")
        }
        "workdeck-launch-video" | "launch-video" => {
            include_str!("../../../skills/workdeck-launch-video/SKILL.md")
        }
        _ => bail!(
            "unknown bundled skill {name:?}; expected workdeck-review, workdeck-extensions, workdeck-release, or workdeck-launch-video"
        ),
    };
    let canonical_name = source
        .lines()
        .find_map(|line| line.strip_prefix("name: "))
        .context("bundled skill is missing its name")?;
    let root = user_config_root()
        .context("could not resolve the Workdeck user config directory")?
        .join("workdeck")
        .join("skills")
        .join(canonical_name);
    std::fs::create_dir_all(&root)
        .with_context(|| format!("failed to create {}", root.display()))?;
    let destination = root.join("SKILL.md");
    if std::fs::read_to_string(&destination).ok().as_deref() != Some(source) {
        let temporary = root.join("SKILL.md.tmp");
        std::fs::write(&temporary, source)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        std::fs::rename(&temporary, &destination)
            .with_context(|| format!("failed to install {}", destination.display()))?;
    }
    if json {
        json_success(
            "skill_path",
            Some("path"),
            json!({ "name": canonical_name, "path": destination }),
        )?;
    } else {
        println!("{}", destination.display());
    }
    Ok(())
}

fn handle_live_session_command(command: LiveSessionCommand) -> Result<()> {
    let directory = default_discovery_directory()
        .context("could not resolve the Workdeck live-session directory")?;
    match command {
        LiveSessionCommand::List { json } => {
            let sessions = SessionClient::discover(&directory);
            let payload = sessions
                .iter()
                .map(safe_session_payload)
                .collect::<Vec<_>>();
            if json {
                json_success("live_session_list", Some("list"), payload)?;
            } else if sessions.is_empty() {
                println!("no live Workdeck review sessions");
            } else {
                for session in sessions {
                    println!(
                        "{}  {}  {}",
                        session.id,
                        session.repo.display(),
                        session.title
                    );
                }
            }
            Ok(())
        }
        LiveSessionCommand::Get { id, repo, json } => {
            let session = resolve_live_session(&directory, id.as_deref(), repo.as_deref())?;
            emit_live_value("live_session", "get", safe_session_payload(&session), json)
        }
        LiveSessionCommand::Context { id, repo, json } => {
            let session = resolve_live_session(&directory, id.as_deref(), repo.as_deref())?;
            let snapshot =
                decode_snapshot(SessionClient::request(&session, SessionAction::Snapshot)?)?;
            let selection = snapshot.selection;
            let file = snapshot.changeset.files.get(selection.file_index);
            let payload = json!({
                "session": safe_session_payload(&session),
                "selection": selection,
                "file": file.map(|file| &file.path),
                "hunk": selection.hunk_index.map(|index| index + 1),
            });
            emit_live_value("live_session_context", "context", payload, json)
        }
        LiveSessionCommand::Review {
            id,
            repo,
            include_patch,
            include_source,
            include_notes,
            json,
        } => {
            let session = resolve_live_session(&directory, id.as_deref(), repo.as_deref())?;
            let snapshot = decode_snapshot(SessionClient::request(
                &session,
                SessionAction::Review {
                    include_patch,
                    include_source,
                    include_agent_context: include_notes,
                },
            )?)?;
            let notes = if include_notes {
                SessionClient::request(&session, SessionAction::CommentList)?
            } else {
                Value::Null
            };
            let payload = json!({
                "session": safe_session_payload(&session),
                "review": snapshot,
                "notes": notes,
            });
            emit_live_value("live_session_review", "review", payload, json)
        }
        LiveSessionCommand::Reload { id, repo, json } => {
            let session = resolve_live_session(&directory, id.as_deref(), repo.as_deref())?;
            let payload = SessionClient::request(&session, SessionAction::Reload)?;
            emit_live_value("live_session_reload", "reload", payload, json)
        }
        LiveSessionCommand::Navigate {
            id,
            repo,
            file,
            hunk,
            old_line,
            new_line,
            json,
        } => {
            let target_count = usize::from(hunk.is_some())
                + usize::from(old_line.is_some())
                + usize::from(new_line.is_some());
            if target_count != 1 {
                bail!("session navigate requires exactly one of --hunk, --old-line, or --new-line");
            }
            let session = resolve_live_session(&directory, id.as_deref(), repo.as_deref())?;
            let snapshot =
                decode_snapshot(SessionClient::request(&session, SessionAction::Snapshot)?)?;
            let file_path = file.to_string_lossy();
            let file_index = snapshot
                .changeset
                .files
                .iter()
                .position(|candidate| {
                    candidate.path == file_path
                        || candidate.previous_path.as_deref() == Some(file_path.as_ref())
                })
                .with_context(|| format!("diff file {file_path:?} is not in the live review"))?;
            let action = if let Some(hunk) = hunk {
                let hunk_index = hunk
                    .checked_sub(1)
                    .context("--hunk is 1-based and must be greater than zero")?;
                SessionAction::NavigateHunk {
                    file_index,
                    hunk_index,
                }
            } else if let Some(line) = old_line {
                SessionAction::RevealLine {
                    file_index,
                    side: ReviewSide::Old,
                    line,
                }
            } else {
                SessionAction::RevealLine {
                    file_index,
                    side: ReviewSide::New,
                    line: new_line.expect("validated one target"),
                }
            };
            let result = SessionClient::request(&session, action)?;
            emit_live_value("live_session_navigation", "navigate", result, json)
        }
        LiveSessionCommand::Comment { command } => handle_live_comment_command(&directory, command),
        LiveSessionCommand::Quit { id, repo, json } => {
            let session = resolve_live_session(&directory, id.as_deref(), repo.as_deref())?;
            let result = SessionClient::request(&session, SessionAction::Quit)?;
            emit_live_value("live_session", "quit", result, json)
        }
    }
}

fn handle_live_comment_command(directory: &Path, command: LiveCommentCommand) -> Result<()> {
    match command {
        LiveCommentCommand::Add {
            id,
            repo,
            file,
            old_line,
            new_line,
            hunk_index,
            summary,
            rationale,
            markup,
            author,
            json,
        } => {
            let has_line_target = old_line.is_some() ^ new_line.is_some();
            if hunk_index.is_some() == has_line_target {
                bail!("comment add requires --hunk-index or exactly one of --old-line/--new-line");
            }
            let session = resolve_live_session(directory, id.as_deref(), repo.as_deref())?;
            let snapshot =
                decode_snapshot(SessionClient::request(&session, SessionAction::Snapshot)?)?;
            let file_path = file.to_string_lossy();
            let target_file = find_diff_file_by_path(&snapshot.changeset.files, &file_path)
                .with_context(|| format!("diff file {file_path:?} is not in the live review"))?;
            let (side, line) = match (old_line, new_line) {
                (Some(line), None) => (Some(ReviewSide::Old), Some(line)),
                (None, Some(line)) => (Some(ReviewSide::New), Some(line)),
                _ => (None, None),
            };
            let input = CommentTargetInput {
                file_path: file_path.into_owned(),
                hunk_index,
                side,
                line,
                summary,
                rationale,
                markup,
                author,
            };
            let target = resolve_comment_target(target_file, &input)?;
            let comment = build_live_comment(
                target_file,
                input,
                live_comment_id(),
                Utc::now().to_rfc3339(),
                target,
            );
            let result = SessionClient::request(
                &session,
                SessionAction::CommentAdd {
                    comment: Box::new(comment.clone()),
                },
            )?;
            emit_live_value(
                "live_comment",
                "add",
                json!({ "comment": comment, "result": result }),
                json,
            )
        }
        LiveCommentCommand::List {
            id,
            repo,
            file,
            json,
        } => {
            let session = resolve_live_session(directory, id.as_deref(), repo.as_deref())?;
            let mut comments: Vec<ReviewComment> = serde_json::from_value(SessionClient::request(
                &session,
                SessionAction::CommentList,
            )?)?;
            if let Some(file) = file {
                let snapshot =
                    decode_snapshot(SessionClient::request(&session, SessionAction::Snapshot)?)?;
                let file_path = file.to_string_lossy();
                let key = snapshot
                    .changeset
                    .files
                    .iter()
                    .find(|candidate| candidate.path == file_path)
                    .map(|file| file.key.as_str())
                    .with_context(|| {
                        format!("diff file {file_path:?} is not in the live review")
                    })?;
                comments.retain(|comment| comment.anchor.file_key == key);
            }
            if json {
                json_success("live_comment_list", Some("list"), comments)?;
            } else if comments.is_empty() {
                println!("no live comments");
            } else {
                for comment in comments {
                    println!("{}  {}", comment.id, comment.summary);
                }
            }
            Ok(())
        }
        LiveCommentCommand::Remove {
            id,
            repo,
            comment_id,
            json,
        } => {
            let session = resolve_live_session(directory, id.as_deref(), repo.as_deref())?;
            let result =
                SessionClient::request(&session, SessionAction::CommentRemove { id: comment_id })?;
            emit_live_value("live_comment", "remove", result, json)
        }
    }
}

fn resolve_live_session(
    directory: &Path,
    id: Option<&str>,
    repo: Option<&Path>,
) -> Result<SessionDescriptor> {
    if id.is_some() && repo.is_some() {
        bail!("choose a session id or --repo, not both");
    }
    let sessions = SessionClient::discover(directory);
    let matches = if let Some(id) = id {
        sessions
            .into_iter()
            .filter(|session| session.id == id)
            .collect::<Vec<_>>()
    } else if let Some(repo) = repo {
        let selector = normalize_session_selector(&SessionSelector {
            repo_root: Some(repo.to_owned()),
            ..SessionSelector::default()
        })?;
        let requested = selector
            .repo_root
            .as_deref()
            .expect("repo selector retains its path");
        let requested = std::fs::canonicalize(requested).unwrap_or_else(|_| requested.to_owned());
        let mut matches = sessions
            .into_iter()
            .filter_map(|session| {
                let root =
                    std::fs::canonicalize(&session.repo).unwrap_or_else(|_| session.repo.clone());
                let selectable = SelectableSession {
                    session_id: session.id.clone(),
                    cwd: root.clone(),
                    repo_root: Some(root),
                };
                repo_selector_distance(&selectable, &requested, None)
                    .map(|distance| (distance, session))
            })
            .collect::<Vec<_>>();
        let closest = matches.iter().map(|(distance, _)| *distance).min();
        matches
            .drain(..)
            .filter(|(distance, _)| Some(*distance) == closest)
            .map(|(_, session)| session)
            .collect::<Vec<_>>()
    } else {
        sessions
    };
    match matches.as_slice() {
        [session] => Ok(session.clone()),
        [] => bail!("no matching live Workdeck review session"),
        _ => bail!("multiple live sessions match; pass a session id or --repo"),
    }
}

fn safe_session_payload(session: &SessionDescriptor) -> Value {
    json!({
        "protocol_version": session.protocol_version,
        "id": session.id,
        "repo": session.repo,
        "title": session.title,
        "process_id": session.process_id,
        "started_at_unix_ms": session.started_at_unix_ms,
    })
}

fn emit_live_value(kind: &str, action: &str, value: Value, json: bool) -> Result<()> {
    if json {
        json_success(kind, Some(action), value)
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
        Ok(())
    }
}

fn live_comment_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("comment-{}-{nanos}", std::process::id())
}

fn handle_extension_command(cwd: &Path, command: ExtensionCommand) -> Result<()> {
    let config_root = user_config_root().context("could not resolve the user config directory")?;
    let extensions_root = config_root.join("workdeck/extensions");
    let manager = ExtensionManager::from_config_root(&config_root);
    match command {
        ExtensionCommand::Install { source, yes, json } => {
            let source = parse_extension_install_source(&source, cwd)?;
            if !yes {
                if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                    bail!(
                        "installing an extension needs confirmation and no terminal is available; re-run with --yes after reviewing {}",
                        source.clone_url
                    );
                }
                println!(
                    "Install {}{}?\nNative extensions run with your full user permissions. Only install repositories you trust.",
                    source.clone_url,
                    source
                        .reference
                        .as_ref()
                        .map_or_else(String::new, |reference| format!(" @ {reference}"))
                );
                print!("Proceed? [y/N] ");
                std::io::stdout()
                    .flush()
                    .context("flush confirmation prompt")?;
                let mut answer = String::new();
                std::io::stdin()
                    .read_line(&mut answer)
                    .context("read extension install confirmation")?;
                if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                    println!("Install cancelled.");
                    return Err(CommandExit(1).into());
                }
            }
            if !json {
                eprintln!(
                    "cloning {}{}…",
                    source.clone_url,
                    source
                        .reference
                        .as_ref()
                        .map_or_else(String::new, |reference| format!(" @ {reference}"))
                );
            }
            let outcome = manager.install(&source)?;
            if json {
                json_success("extension_install", Some("install"), &outcome)?;
            } else {
                println!(
                    "Installed {}{} at {} into {}.\nNew Workdeck sessions will load it automatically.",
                    outcome.name,
                    outcome
                        .version
                        .as_ref()
                        .map_or_else(String::new, |version| format!(" v{version}")),
                    short_commit(&outcome.commit),
                    outcome.directory.display()
                );
            }
            Ok(())
        }
        ExtensionCommand::List { json } => {
            let trust = load_extension_trust_store();
            let repo = AnyProvider::discover(cwd, ProviderPreference::Auto)
                .ok()
                .map(|provider| provider.root().to_owned())
                .or_else(|| find_project_root_candidate(cwd));
            let config = Config::load(repo.as_deref().unwrap_or(cwd))?;
            let manifests = discover_manifests_with_config(
                Some(&extensions_root),
                repo.as_deref(),
                &trust,
                &[],
                &config.resolved_extensions.paths,
                &config.resolved_extensions.repo_paths,
                cwd,
            )?
            .manifests;
            let managed = manager.list();
            let mut seen_managed = BTreeSet::new();
            let mut payload = manifests
                .iter()
                .map(|path| {
                    let manifest = ExtensionManifest::load(path);
                    let managed_entry = managed.iter().find(|entry| {
                        path.strip_prefix(manager.installed_root())
                            .ok()
                            .and_then(|relative| relative.components().next())
                            .is_some_and(|component| component.as_os_str() == entry.name.as_str())
                    });
                    if let Some(entry) = managed_entry {
                        seen_managed.insert(entry.name.clone());
                    }
                    json!({
                        "path": path,
                        "valid": manifest.is_ok(),
                        "id": manifest.as_ref().ok().map(|manifest| &manifest.id),
                        "version": manifest.as_ref().ok().map(|manifest| &manifest.version),
                        "error": manifest.err().map(|error| error.to_string()),
                        "managed": managed_entry.is_some(),
                        "managed_name": managed_entry.map(|entry| &entry.name),
                        "source": managed_entry.map(|entry| &entry.record.clone_url),
                        "ref": managed_entry.and_then(|entry| entry.record.reference.as_deref()),
                        "commit": managed_entry.map(|entry| &entry.record.commit),
                    })
                })
                .collect::<Vec<_>>();
            payload.extend(
                managed
                    .iter()
                    .filter(|entry| !seen_managed.contains(&entry.name))
                    .map(|entry| {
                        json!({
                            "path": entry.directory,
                            "valid": false,
                            "id": entry.name,
                            "version": entry.version,
                            "error": "managed extension is missing on disk",
                            "managed": true,
                            "managed_name": entry.name,
                            "source": entry.record.clone_url,
                            "ref": entry.record.reference,
                            "commit": entry.record.commit,
                        })
                    }),
            );
            if json {
                json_success("extension_list", Some("list"), payload)?;
            } else if payload.is_empty() {
                println!(
                    "No native Workdeck extensions discovered.\nInstall one with `workdeck extension install <owner>/<repo>`."
                );
            } else {
                for extension in payload {
                    let missing = extension["error"]
                        .as_str()
                        .filter(|error| *error == "managed extension is missing on disk")
                        .map_or("", |_| "  (missing on disk — reinstall or remove)");
                    println!(
                        "{:<24} {}{}",
                        extension["id"].as_str().unwrap_or("invalid"),
                        extension["path"].as_str().unwrap_or_default(),
                        missing
                    );
                }
            }
            Ok(())
        }
        ExtensionCommand::Update { name, json } => {
            let outcomes = if let Some(name) = name {
                if !json {
                    eprintln!("checking {name}…");
                }
                vec![manager.update(&name)?]
            } else {
                let entries = manager.list();
                if entries.is_empty() {
                    if json {
                        json_success("extension_update", Some("update"), Vec::<Value>::new())?;
                    } else {
                        println!("No managed native extensions to update.");
                    }
                    return Ok(());
                }
                if !json {
                    for entry in &entries {
                        eprintln!("checking {}…", entry.name);
                    }
                }
                manager.update_all()?
            };
            if json {
                json_success("extension_update", Some("update"), &outcomes)?;
            } else {
                for outcome in outcomes {
                    if outcome.changed {
                        println!(
                            "Updated {}{}: {} -> {}.",
                            outcome.install.name,
                            outcome
                                .install
                                .version
                                .as_ref()
                                .map_or_else(String::new, |version| format!(" to v{version}")),
                            short_commit(&outcome.previous_commit),
                            short_commit(&outcome.install.commit)
                        );
                    } else {
                        println!(
                            "{} is already up to date ({}).",
                            outcome.install.name,
                            short_commit(&outcome.install.commit)
                        );
                    }
                }
            }
            Ok(())
        }
        ExtensionCommand::Remove { name, json } => {
            let record = manager.remove(&name)?;
            if json {
                json_success(
                    "extension_remove",
                    Some("remove"),
                    json!({ "name": name, "record": record }),
                )?;
            } else {
                println!("Removed {name}.");
            }
            Ok(())
        }
        ExtensionCommand::Validate { path, json } => {
            let path = if path.is_dir() {
                path.join("workdeck-extension.toml")
            } else {
                path
            };
            let manifest = ExtensionManifest::load(&path)?;
            if json {
                json_success(
                    "extension_manifest",
                    Some("validate"),
                    json!({ "path": path, "manifest": manifest }),
                )?;
            } else {
                println!(
                    "valid native extension {} {} (API {})",
                    manifest.id, manifest.version, manifest.api_version
                );
            }
            Ok(())
        }
        ExtensionCommand::Trust {
            mut repo,
            allow,
            deny,
            yes,
            json,
        } => {
            if allow == deny {
                bail!("extension trust requires exactly one of --allow or --deny");
            }
            if !yes {
                bail!("native extensions run with your user permissions; pass --yes to confirm");
            }
            let state_path = resolve_app_state_path()
                .context("could not resolve the Workdeck app-state path")?;
            if !repo.is_absolute() {
                repo = cwd.join(repo);
            }
            let mut trust = load_extension_trust_store();
            let decision = if allow {
                TrustDecision::Trusted
            } else {
                TrustDecision::Denied
            };
            trust.grant(&repo, decision);
            workdeck_store::update_app_state_record(&state_path, trust.app_state_patch())?;
            if json {
                json_success(
                    "extension_trust",
                    Some("set"),
                    json!({ "repo": repo, "decision": decision }),
                )?;
            } else {
                println!(
                    "recorded {decision:?} extension trust for {}",
                    repo.display()
                );
            }
            Ok(())
        }
    }
}

fn short_commit(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

fn handle_migrate_command(cwd: &Path, command: MigrateCommand) -> Result<()> {
    match command {
        MigrateCommand::Hunk {
            dry_run: _,
            apply,
            json,
        } => {
            let plan = workdeck_migration::plan(cwd)?;
            if !apply {
                if json {
                    json_success("hunk_migration", Some("dry_run"), &plan)?;
                } else {
                    println!("Hunk migration dry run");
                    if let Some(global) = &plan.global {
                        println!(
                            "  global: {} -> {}",
                            global.source.display(),
                            global.destination.display()
                        );
                    }
                    if let Some(repository) = &plan.repository {
                        println!(
                            "  repository: {} -> {}",
                            repository.source.display(),
                            repository.destination.display()
                        );
                    }
                    println!("  legacy extensions: {}", plan.legacy_extensions.len());
                    for warning in &plan.warnings {
                        println!("  warning: {warning}");
                    }
                    println!("Run `workdeck migrate hunk --apply` to write the migration.");
                }
                return Ok(());
            }
            let result = workdeck_migration::apply(&plan)?;
            if json {
                json_success("hunk_migration", Some("apply"), result)?;
            } else if result.changed.is_empty() {
                println!("Hunk migration already applied; no files changed");
            } else {
                for path in result.changed {
                    println!("migrated {}", path.display());
                }
                for backup in result.backups {
                    println!("backup {}", backup.display());
                }
            }
            Ok(())
        }
    }
}

fn user_config_root() -> Option<PathBuf> {
    workdeck_core::resolve_user_config_dir()
}

fn load_extension_trust_store() -> TrustStore {
    let mut trust = user_config_root()
        .map(|root| TrustStore::load_legacy_toml(&root.join("workdeck/extension-trust.toml")))
        .unwrap_or_default();
    if let Some(state_path) = resolve_app_state_path() {
        trust.merge_app_state_record(&workdeck_store::read_app_state_record(state_path));
    }
    trust
}

impl FilesCommand {
    fn wants_json(&self) -> bool {
        match self {
            FilesCommand::List { json, .. } | FilesCommand::Show { json, .. } => *json,
        }
    }
}

impl ChangesCommand {
    fn wants_json(&self) -> bool {
        match self {
            ChangesCommand::List { json, .. } | ChangesCommand::Diff { json, .. } => *json,
        }
    }
}

impl ConfigCommand {
    fn wants_json(&self) -> bool {
        match self {
            ConfigCommand::Path { json }
            | ConfigCommand::Show { json }
            | ConfigCommand::Init { json }
            | ConfigCommand::Validate { json }
            | ConfigCommand::Get { json, .. }
            | ConfigCommand::Set { json, .. } => *json,
        }
    }
}

impl EventsCommand {
    fn wants_json(&self) -> bool {
        match self {
            EventsCommand::List { json } => *json,
        }
    }
}

impl IssueCommand {
    fn wants_json(&self) -> bool {
        match self {
            IssueCommand::List { json, .. }
            | IssueCommand::Create { json, .. }
            | IssueCommand::Update { json, .. }
            | IssueCommand::Link { json, .. }
            | IssueCommand::LinkFile { json, .. }
            | IssueCommand::UnlinkFile { json, .. }
            | IssueCommand::LinkCommit { json, .. }
            | IssueCommand::UnlinkCommit { json, .. }
            | IssueCommand::Close { json, .. }
            | IssueCommand::Reopen { json, .. }
            | IssueCommand::Move { json, .. }
            | IssueCommand::Assign { json, .. }
            | IssueCommand::Unassign { json, .. }
            | IssueCommand::Delete { json, .. }
            | IssueCommand::Show { json, .. } => *json,
            IssueCommand::Label { command } => command.wants_json(),
        }
    }
}

impl IssueLabelCommand {
    fn wants_json(&self) -> bool {
        match self {
            IssueLabelCommand::Add { json, .. } | IssueLabelCommand::Remove { json, .. } => *json,
        }
    }
}

impl AgentCommand {
    fn wants_json(&self) -> bool {
        match self {
            AgentCommand::List { json }
            | AgentCommand::Record { json, .. }
            | AgentCommand::Show { json, .. }
            | AgentCommand::Update { json, .. }
            | AgentCommand::Finish { json, .. }
            | AgentCommand::AppendPlan { json, .. }
            | AgentCommand::AddFile { json, .. }
            | AgentCommand::AddCommand { json, .. }
            | AgentCommand::AddTest { json, .. }
            | AgentCommand::AddNote { json, .. }
            | AgentCommand::Delete { json, .. }
            | AgentCommand::Import { json, .. } => *json,
        }
    }
}

impl ProjectCommand {
    fn wants_json(&self) -> bool {
        match self {
            ProjectCommand::List { json, .. }
            | ProjectCommand::Save { json, .. }
            | ProjectCommand::Show { json, .. }
            | ProjectCommand::Delete { json, .. } => *json,
        }
    }
}

impl CycleCommand {
    fn wants_json(&self) -> bool {
        match self {
            CycleCommand::List { json, .. }
            | CycleCommand::Save { json, .. }
            | CycleCommand::Show { json, .. }
            | CycleCommand::Delete { json, .. } => *json,
        }
    }
}

impl LabelCommand {
    fn wants_json(&self) -> bool {
        match self {
            LabelCommand::List { json, .. }
            | LabelCommand::Save { json, .. }
            | LabelCommand::Show { json, .. }
            | LabelCommand::Delete { json, .. } => *json,
        }
    }
}

fn classify_exit_code(error: &anyhow::Error) -> u8 {
    if let Some(exit) = error.downcast_ref::<CommandExit>() {
        return u8::try_from(exit.0).unwrap_or(1).max(1);
    }
    let message = format!("{error:#}").to_ascii_lowercase();
    if message.contains("__json_error_printed__") {
        1
    } else if message.contains("does not exist") || message.contains("not found") {
        3
    } else if message.contains("duplicate") || message.contains("already exists") {
        4
    } else if message.contains("failed to parse")
        || message.contains("invalid workdeck config")
        || message.contains("config")
    {
        5
    } else if message.contains("unknown ")
        || message.contains("invalid ")
        || message.contains("cannot ")
        || message.contains("requires --yes")
        || message.contains("must ")
    {
        2
    } else {
        1
    }
}

fn is_json_error_already_printed(error: &anyhow::Error) -> bool {
    format!("{error:#}").contains("__json_error_printed__")
}

fn json_success(kind: &str, action: Option<&str>, data: impl serde::Serialize) -> Result<()> {
    let mut payload = json!({
        "ok": true,
        "kind": kind,
        "data": data,
    });
    if let Some(action) = action {
        payload["action"] = json!(action);
    }
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

fn print_json_error(error: &anyhow::Error) -> Result<()> {
    let code = classify_exit_code(error);
    let code_name = match code {
        2 => "validation_error",
        3 => "not_found",
        4 => "conflict",
        5 => "config_or_store_error",
        _ => "error",
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": false,
            "error": {
                "code": code_name,
                "message": format!("{error:#}"),
            }
        }))?
    );
    Ok(())
}

fn handle_export(
    repo_root: &Path,
    store: &WorkdeckStore,
    json_output: bool,
    jsonl: bool,
) -> Result<()> {
    let issues = store.load_issues()?;
    let reference_data = store.load_reference_data()?;
    let sessions = store.load_agent_sessions()?;
    let events = store.load_events()?;

    if jsonl {
        print_jsonl_record("repo", json!({ "root": repo_root }))?;
        for issue in issues {
            print_jsonl_record("issue", serde_json::to_value(issue)?)?;
        }
        for project in reference_data.projects {
            print_jsonl_record("project", serde_json::to_value(project)?)?;
        }
        for cycle in reference_data.cycles {
            print_jsonl_record("cycle", serde_json::to_value(cycle)?)?;
        }
        for label in reference_data.labels {
            print_jsonl_record("label", serde_json::to_value(label)?)?;
        }
        for session in sessions {
            print_jsonl_record("agent_session", serde_json::to_value(session)?)?;
        }
        for event in events {
            print_event_jsonl_record(event)?;
        }
    } else {
        let payload = json!({
            "repo_root": repo_root,
            "issues": issues,
            "projects": reference_data.projects,
            "cycles": reference_data.cycles,
            "labels": reference_data.labels,
            "agent_sessions": sessions,
            "events": events,
        });
        if json_output {
            json_success("export", None, payload)?;
        } else {
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }

    Ok(())
}

fn print_status(repo_root: &Path, json_output: bool) -> Result<()> {
    let snapshot = git::scan_repo(repo_root)?;
    let payload = status_payload(&snapshot);
    if json_output {
        json_success("status", None, payload)?;
    } else if snapshot.changes.is_empty() {
        println!("clean worktree");
    } else {
        for change in &snapshot.changes {
            println!(
                "{:<12} {:<18} {:>5} {:>5} {}",
                change.kind.label(),
                change.stage_label(),
                format!("+{}", change.additions),
                format!("-{}", change.deletions),
                change.path.display()
            );
        }
    }
    Ok(())
}

fn handle_files_command(repo_root: &Path, command: FilesCommand) -> Result<()> {
    match command {
        FilesCommand::List { path, json } => {
            let files = git::list_repo_files(repo_root, 20_000)?;
            let entries = file_entries_for_path(&files, path.as_deref().unwrap_or(Path::new("")));
            if json {
                json_success("file_list", None, entries)?;
            } else if entries.is_empty() {
                println!("no files");
            } else {
                for entry in entries {
                    println!(
                        "{:<9} {}",
                        entry["kind"].as_str().unwrap_or("unknown"),
                        entry["path"].as_str().unwrap_or("")
                    );
                }
            }
        }
        FilesCommand::Show { path, json } => {
            let preview = git::read_file_preview(repo_root, &path, 80_000)?;
            if json {
                json_success("file_preview", None, file_preview_payload(&preview))?;
            } else {
                print!("{}", preview.content);
            }
        }
    }
    Ok(())
}

fn file_entries_for_path(files: &[PathBuf], cwd: &Path) -> Vec<Value> {
    let mut dirs = BTreeSet::<PathBuf>::new();
    let mut direct_files = Vec::<PathBuf>::new();
    for path in files {
        let relative = if cwd.as_os_str().is_empty() {
            path.as_path()
        } else {
            match path.strip_prefix(cwd) {
                Ok(relative) if !relative.as_os_str().is_empty() => relative,
                _ => continue,
            }
        };
        let mut components = relative.components();
        let Some(first) = components.next() else {
            continue;
        };
        let child = cwd.join(first.as_os_str());
        if components.next().is_some() {
            dirs.insert(child);
        } else {
            direct_files.push(child);
        }
    }
    direct_files.sort();

    dirs.into_iter()
        .map(|path| json!({ "kind": "directory", "path": path, "name": file_name(&path) }))
        .chain(
            direct_files
                .into_iter()
                .map(|path| json!({ "kind": "file", "path": path, "name": file_name(&path) })),
        )
        .collect()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn handle_changes_command(repo_root: &Path, command: ChangesCommand) -> Result<()> {
    match command {
        ChangesCommand::List { group, json } => {
            let snapshot = git::scan_repo(repo_root)?;
            if json {
                let payload = match group.as_str() {
                    "directory" | "dir" => status_payload(&snapshot),
                    "status" => json!({
                        "repo_root": snapshot.root,
                        "groups": changes_grouped_by_status(&snapshot.changes),
                    }),
                    value => bail!("unknown change group {value}"),
                };
                json_success("change_list", None, payload)?;
            } else if snapshot.changes.is_empty() {
                println!("clean worktree");
            } else {
                match group.as_str() {
                    "directory" | "dir" => {
                        for group in snapshot.groups {
                            println!(
                                "{} {} +{} -{}",
                                group.path.display(),
                                group.files.len(),
                                group.total_additions,
                                group.total_deletions
                            );
                            for change in group.files {
                                println!("  {:<10} {}", change.kind.label(), change.path.display());
                            }
                        }
                    }
                    "status" => {
                        for group in changes_grouped_by_status(&snapshot.changes) {
                            println!("{}", group["status"].as_str().unwrap_or("unknown"));
                            for change in group["changes"].as_array().into_iter().flatten() {
                                println!("  {}", change["path"].as_str().unwrap_or_default());
                            }
                        }
                    }
                    value => bail!("unknown change group {value}"),
                }
            }
        }
        ChangesCommand::Diff { path, json } => {
            let preview = git::diff_for_path(repo_root, &path)?;
            if json {
                json_success("change_diff", None, file_preview_payload(&preview))?;
            } else {
                print!("{}", preview.content);
            }
        }
    }
    Ok(())
}

fn handle_search_command(
    repo_root: &Path,
    config: &Config,
    store: &WorkdeckStore,
    query: String,
    targets: Vec<String>,
    json_output: bool,
) -> Result<()> {
    let snapshot = git::scan_repo(repo_root)?;
    let git_overview = git::scan_git_overview(
        repo_root,
        Some(&config.git.base_branch),
        config.git.recent_commits,
    )?;
    let files = git::list_repo_files(repo_root, 20_000)?;
    let issues = store.load_issues()?;
    let sessions = store.load_agent_sessions()?;
    let references = store.load_reference_data()?;
    let symbols = workdeck_cli::search::extract_symbols(repo_root, &files);
    let index = workdeck_cli::search::SearchIndex::rebuild(
        &files,
        &snapshot.changes,
        &issues,
        &sessions,
        &references,
        &symbols,
        Some(&git_overview),
    );
    let target_filter = targets
        .into_iter()
        .map(|target| target.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let results = index
        .query(&query, 100)
        .into_iter()
        .filter(|result| {
            target_filter.is_empty()
                || target_filter.contains(search_target_group(&result.record.target))
        })
        .map(|result| {
            json!({
                "score": result.score,
                "label": result.record.label,
                "detail": result.record.detail,
                "target": search_target_payload(&result.record.target),
            })
        })
        .collect::<Vec<_>>();

    if json_output {
        json_success("search_results", None, results)?;
    } else if results.is_empty() {
        println!("no results");
    } else {
        for result in results {
            println!(
                "{:<6} {:<10} {}",
                result["score"].as_i64().unwrap_or_default(),
                result["target"]["kind"].as_str().unwrap_or("unknown"),
                result["label"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn print_jsonl_record(kind: &str, payload: Value) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "kind": kind,
            "payload": payload,
        }))?
    );
    Ok(())
}

fn print_event_jsonl_record(event: StoreEvent) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&json!({
            "kind": "event",
            "payload": {
                "kind": event.kind,
                "payload": event.payload,
                "created_at": event.created_at,
            },
        }))?
    );
    Ok(())
}

fn handle_config_command(
    repo_root: &Path,
    store: &WorkdeckStore,
    command: ConfigCommand,
) -> Result<()> {
    let path = repo_root.join(".agents/workdeck/config.toml");
    match command {
        ConfigCommand::Path { json } => {
            if json {
                json_success("config_path", None, json!({ "path": path }))?;
            } else {
                println!("{}", path.display());
            }
        }
        ConfigCommand::Show { json } => {
            let config = Config::load(repo_root)?;
            if json {
                json_success("config", None, config)?;
            } else {
                println!("{}", toml::to_string_pretty(&config)?);
            }
        }
        ConfigCommand::Init { json } => {
            store.init()?;
            if json {
                json_success(
                    "config",
                    Some("init"),
                    json!({ "initialized": true, "path": store.root() }),
                )?;
            } else {
                println!("initialized {}", store.root().display());
            }
        }
        ConfigCommand::Validate { json } => {
            Config::load(repo_root)?;
            if json {
                json_success("config_validation", None, json!({ "ok": true }))?;
            } else {
                println!("config ok");
            }
        }
        ConfigCommand::Get { key, json } => {
            let value = load_repo_config_value(&path)?;
            let Some(value) = get_config_value(&value, &key) else {
                bail!("config key {key} does not exist");
            };
            if json {
                json_success("config_value", None, json!({ "key": key, "value": value }))?;
            } else {
                println!("{value}");
            }
        }
        ConfigCommand::Set { key, value, json } => {
            let mut root = load_repo_config_value(&path)?;
            let storage_key = config_storage_key(&key);
            set_toml_path(&mut root, &storage_key, parse_config_value(&value))?;
            let raw = toml::to_string_pretty(&root)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, raw)?;
            Config::load(repo_root)?;
            if json {
                json_success(
                    "config",
                    Some("set"),
                    json!({ "key": storage_key, "set": true }),
                )?;
            } else {
                println!("set {storage_key}");
            }
        }
    }
    Ok(())
}

fn load_repo_config_value(path: &Path) -> Result<toml::Value> {
    if !path.exists() {
        return Ok(toml::Value::Table(Default::default()));
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn get_toml_path<'a>(value: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    let mut current = value;
    for part in key.split('.') {
        current = current.as_table()?.get(part)?;
    }
    Some(current)
}

fn get_config_value<'a>(value: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    get_toml_path(value, key).or_else(|| {
        if key == "keys.issues" {
            get_toml_path(value, "keys.tasks")
        } else {
            None
        }
    })
}

fn config_storage_key(key: &str) -> String {
    if key == "keys.tasks" {
        "keys.issues".to_string()
    } else {
        key.to_string()
    }
}

fn set_toml_path(root: &mut toml::Value, key: &str, value: toml::Value) -> Result<()> {
    let parts = key
        .split('.')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        bail!("config key cannot be empty");
    }
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        let table = current
            .as_table_mut()
            .with_context(|| format!("config path {key} is not a table"))?;
        current = table
            .entry((*part).to_string())
            .or_insert_with(|| toml::Value::Table(Default::default()));
    }
    current
        .as_table_mut()
        .with_context(|| format!("config path {key} is not a table"))?
        .insert(parts[parts.len() - 1].to_string(), value);
    Ok(())
}

fn parse_config_value(value: &str) -> toml::Value {
    match value {
        "true" => toml::Value::Boolean(true),
        "false" => toml::Value::Boolean(false),
        _ => toml::Value::String(value.to_string()),
    }
}

fn handle_events_command(store: &WorkdeckStore, command: EventsCommand) -> Result<()> {
    match command {
        EventsCommand::List { json } => {
            let events = store.load_events()?;
            if json {
                json_success("event_list", None, events)?;
            } else if events.is_empty() {
                println!("no events");
            } else {
                for event in events {
                    println!("{:<24} {}", event.created_at, event.kind);
                }
            }
        }
    }
    Ok(())
}

fn handle_import_command(
    store: &WorkdeckStore,
    path: PathBuf,
    _merge: bool,
    replace: bool,
    dry_run: bool,
    json_output: bool,
) -> Result<()> {
    let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    let value: Value = serde_json::from_reader(file)
        .with_context(|| format!("failed to parse JSON {}", path.display()))?;
    let issues: Vec<Issue> = serde_json::from_value(
        value
            .get("issues")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse issues")?;
    let projects: Vec<Project> = serde_json::from_value(
        value
            .get("projects")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse projects")?;
    let cycles: Vec<Cycle> = serde_json::from_value(
        value
            .get("cycles")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse cycles")?;
    let labels: Vec<Label> = serde_json::from_value(
        value
            .get("labels")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse labels")?;
    let sessions: Vec<AgentSession> = serde_json::from_value(
        value
            .get("agent_sessions")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    )
    .with_context(|| "failed to parse agent sessions")?;

    if !dry_run {
        if replace && store.root().exists() {
            std::fs::remove_dir_all(store.root())
                .with_context(|| format!("failed to replace {}", store.root().display()))?;
        }
        store.init()?;
        for issue in &issues {
            store.save_issue(issue)?;
        }
        for project in &projects {
            store.upsert_project(
                Some(project.id.clone()),
                project.name.clone(),
                Some(project.description.clone()),
                Some(project.status.clone()),
            )?;
        }
        for cycle in &cycles {
            store.upsert_cycle(
                Some(cycle.id.clone()),
                cycle.name.clone(),
                Some(cycle.starts_at.clone()),
                Some(cycle.ends_at.clone()),
                Some(cycle.status.clone()),
            )?;
        }
        for label in &labels {
            store.upsert_label(
                Some(label.id.clone()),
                label.name.clone(),
                Some(label.color.clone()),
            )?;
        }
        for session in &sessions {
            store.save_agent_session(session)?;
        }
        store.append_event(
            "import_completed",
            json!({
                "path": path,
                "replace": replace,
            }),
        )?;
    }

    let payload = json!({
        "dry_run": dry_run,
        "issues": issues.len(),
        "projects": projects.len(),
        "cycles": cycles.len(),
        "labels": labels.len(),
        "agent_sessions": sessions.len(),
    });
    if json_output {
        json_success(
            "import",
            Some(if dry_run { "dry-run" } else { "import" }),
            payload,
        )?;
    } else if dry_run {
        println!("import dry-run ok");
    } else {
        println!("imported Workdeck data");
    }
    Ok(())
}

fn handle_doctor(repo_root: &std::path::Path, as_json: bool) -> Result<()> {
    let config_result = Config::load(repo_root);
    let (data_dir, config_check) = match config_result {
        Ok(config) => (
            config.data_dir(repo_root),
            json!({
                "name": "config",
                "ok": true,
                "message": "config is valid",
            }),
        ),
        Err(error) => (
            Config::default().data_dir(repo_root),
            json!({
                "name": "config",
                "ok": false,
                "message": format!("{error:#}"),
            }),
        ),
    };
    let store = WorkdeckStore::new(data_dir.clone());

    let checks = vec![
        json!({
            "name": "repo",
            "ok": repo_root.join(".git").exists(),
            "message": repo_root.display().to_string(),
        }),
        config_check,
        json!({
            "name": "data_dir",
            "ok": true,
            "message": data_dir.display().to_string(),
            "exists": data_dir.exists(),
        }),
        doctor_check("issues", store.load_issues(), |issues| {
            format!("{} issue(s)", issues.len())
        }),
        doctor_check("agents", store.load_agent_sessions(), |sessions| {
            format!("{} agent session(s)", sessions.len())
        }),
        doctor_check(
            "references",
            store.load_reference_data(),
            |reference_data| reference_summary(&reference_data),
        ),
        doctor_check("events", store.load_events(), |events| {
            format!("{} event(s)", events.len())
        }),
    ];
    let ok = checks
        .iter()
        .all(|check| check["ok"].as_bool().unwrap_or(false));

    if as_json {
        if ok {
            json_success("doctor", None, json!({ "ok": ok, "checks": checks }))?;
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "ok": false,
                    "kind": "doctor",
                    "error": {
                        "code": "doctor_failed",
                        "message": "doctor found failed checks",
                    },
                    "data": {
                        "checks": checks,
                    }
                }))?
            );
            bail!("__json_error_printed__: doctor found failed checks");
        }
    } else {
        for check in checks {
            let mark = if check["ok"].as_bool().unwrap_or(false) {
                "ok"
            } else {
                "fail"
            };
            println!(
                "{:<5} {:<10} {}",
                mark,
                check["name"].as_str().unwrap_or("unknown"),
                check["message"].as_str().unwrap_or("")
            );
        }
    }

    if !ok {
        bail!("doctor found failed checks");
    }
    Ok(())
}

fn doctor_check<T>(
    name: &str,
    result: Result<T>,
    message: impl FnOnce(T) -> String,
) -> serde_json::Value {
    match result {
        Ok(value) => json!({
            "name": name,
            "ok": true,
            "message": message(value),
        }),
        Err(error) => json!({
            "name": name,
            "ok": false,
            "message": format!("{error:#}"),
        }),
    }
}

fn handle_project_command(store: &WorkdeckStore, command: ProjectCommand) -> Result<()> {
    match command {
        ProjectCommand::List { status, json } => {
            let mut projects = store.load_reference_data()?.projects;
            if let Some(status) = status {
                projects.retain(|project| project.status == status);
            }
            if json {
                json_success("project_list", None, projects)?;
            } else if projects.is_empty() {
                println!("no projects");
            } else {
                for project in projects {
                    println!("{:<24} {:<10} {}", project.id, project.status, project.name);
                }
            }
        }
        ProjectCommand::Save {
            name,
            id,
            description,
            status,
            json,
        } => {
            let project = store.upsert_project(id, name, description, status)?;
            if json {
                json_success("project", Some("save"), project)?;
            } else {
                println!("{} {}", project.id, project.name);
            }
        }
        ProjectCommand::Show { id, json } => {
            let project = store
                .load_reference_data()?
                .projects
                .into_iter()
                .find(|project| project.id == id)
                .with_context(|| format!("project {id} does not exist"))?;
            if json {
                json_success("project", None, project)?;
            } else {
                println!("{} {}", project.id, project.name);
            }
        }
        ProjectCommand::Delete {
            id,
            yes,
            force,
            json,
        } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let project = store.delete_project(&id, force)?;
            if json {
                json_success("project", Some("delete"), project)?;
            } else {
                println!("deleted {}", project.id);
            }
        }
    }
    Ok(())
}

fn handle_cycle_command(store: &WorkdeckStore, command: CycleCommand) -> Result<()> {
    match command {
        CycleCommand::List { status, json } => {
            let mut cycles = store.load_reference_data()?.cycles;
            if let Some(status) = status {
                cycles.retain(|cycle| cycle.status == status);
            }
            if json {
                json_success("cycle_list", None, cycles)?;
            } else if cycles.is_empty() {
                println!("no cycles");
            } else {
                for cycle in cycles {
                    println!("{:<24} {:<10} {}", cycle.id, cycle.status, cycle.name);
                }
            }
        }
        CycleCommand::Save {
            name,
            id,
            starts_at,
            ends_at,
            status,
            json,
        } => {
            let cycle = store.upsert_cycle(id, name, starts_at, ends_at, status)?;
            if json {
                json_success("cycle", Some("save"), cycle)?;
            } else {
                println!("{} {}", cycle.id, cycle.name);
            }
        }
        CycleCommand::Show { id, json } => {
            let cycle = store
                .load_reference_data()?
                .cycles
                .into_iter()
                .find(|cycle| cycle.id == id)
                .with_context(|| format!("cycle {id} does not exist"))?;
            if json {
                json_success("cycle", None, cycle)?;
            } else {
                println!("{} {}", cycle.id, cycle.name);
            }
        }
        CycleCommand::Delete {
            id,
            yes,
            force,
            json,
        } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let cycle = store.delete_cycle(&id, force)?;
            if json {
                json_success("cycle", Some("delete"), cycle)?;
            } else {
                println!("deleted {}", cycle.id);
            }
        }
    }
    Ok(())
}

fn handle_label_command(store: &WorkdeckStore, command: LabelCommand) -> Result<()> {
    match command {
        LabelCommand::List { color, json } => {
            let mut labels = store.load_reference_data()?.labels;
            if let Some(color) = color {
                labels.retain(|label| label.color == color);
            }
            if json {
                json_success("label_list", None, labels)?;
            } else if labels.is_empty() {
                println!("no labels");
            } else {
                for label in labels {
                    println!("{:<24} {:<10} {}", label.id, label.color, label.name);
                }
            }
        }
        LabelCommand::Save {
            name,
            id,
            color,
            json,
        } => {
            let label = store.upsert_label(id, name, color)?;
            if json {
                json_success("label", Some("save"), label)?;
            } else {
                println!("{} {}", label.id, label.name);
            }
        }
        LabelCommand::Show { id, json } => {
            let label = store
                .load_reference_data()?
                .labels
                .into_iter()
                .find(|label| label.id == id)
                .with_context(|| format!("label {id} does not exist"))?;
            if json {
                json_success("label", None, label)?;
            } else {
                println!("{} {}", label.id, label.name);
            }
        }
        LabelCommand::Delete {
            id,
            yes,
            force,
            json,
        } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let label = store.delete_label(&id, force)?;
            if json {
                json_success("label", Some("delete"), label)?;
            } else {
                println!("deleted {}", label.id);
            }
        }
    }
    Ok(())
}

fn reference_summary(reference_data: &ReferenceData) -> String {
    format!(
        "{} project(s), {} cycle(s), {} label(s)",
        reference_data.projects.len(),
        reference_data.cycles.len(),
        reference_data.labels.len()
    )
}

fn handle_agent_command(store: &WorkdeckStore, command: AgentCommand) -> Result<()> {
    match command {
        AgentCommand::List { json } => {
            let sessions = store.load_agent_sessions()?;
            if json {
                json_success("agent_session_list", None, sessions)?;
            } else if sessions.is_empty() {
                println!("no agent sessions");
            } else {
                for session in sessions {
                    println!(
                        "{:<32} {:<10} {:<10} {}",
                        session.id, session.agent, session.status, session.title
                    );
                }
            }
        }
        AgentCommand::Record {
            title,
            id,
            agent,
            status,
            goal,
            summary,
            cwd,
            plan_item,
            touched_file,
            command_run,
            test_run,
            handoff_note,
            json,
        } => {
            let mut session = AgentSession::new(title);
            if let Some(id) = id {
                session.id = id;
            }
            if let Some(agent) = agent {
                session.agent = agent;
            }
            if let Some(status) = status {
                session.status = status;
            }
            if let Some(goal) = goal {
                session.goal = goal;
            }
            if let Some(summary) = summary {
                session.summary = summary;
            }
            if let Some(cwd) = cwd {
                session.cwd = cwd.display().to_string();
            }
            session.plan = plan_item;
            session.touched_files = touched_file
                .into_iter()
                .map(|path| AgentTouchedFile {
                    path,
                    change_type: String::new(),
                })
                .collect();
            session.commands_run = command_run;
            session.tests_run = test_run;
            session.handoff_notes = handoff_note;
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("record"))?;
        }
        AgentCommand::Show { id, json } => {
            let session = load_agent_session(store, &id)?;
            print_agent_session(session, json)?;
        }
        AgentCommand::Update {
            id,
            title,
            agent,
            status,
            goal,
            summary,
            cwd,
            json,
        } => {
            let mut session = load_agent_session(store, &id)?;
            if let Some(title) = title {
                session.title = title;
            }
            if let Some(agent) = agent {
                session.agent = agent;
            }
            if let Some(status) = status {
                session.status = status;
            }
            if let Some(goal) = goal {
                session.goal = goal;
            }
            if let Some(summary) = summary {
                session.summary = summary;
            }
            if let Some(cwd) = cwd {
                session.cwd = cwd.display().to_string();
            }
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("update"))?;
        }
        AgentCommand::Finish { id, summary, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.status = "done".to_string();
            session.ended_at = Utc::now().to_rfc3339();
            if let Some(summary) = summary {
                session.summary = summary;
            }
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("finish"))?;
        }
        AgentCommand::AppendPlan { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.plan.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("append-plan"))?;
        }
        AgentCommand::AddFile {
            id,
            path,
            change_type,
            json,
        } => {
            let mut session = load_agent_session(store, &id)?;
            session
                .touched_files
                .push(AgentTouchedFile { path, change_type });
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-file"))?;
        }
        AgentCommand::AddCommand { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.commands_run.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-command"))?;
        }
        AgentCommand::AddTest { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.tests_run.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-test"))?;
        }
        AgentCommand::AddNote { id, text, json } => {
            let mut session = load_agent_session(store, &id)?;
            session.handoff_notes.push(text);
            store.save_agent_session(&session)?;
            print_agent_session_action(session, json, Some("add-note"))?;
        }
        AgentCommand::Delete { id, yes, json } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let path = store.agent_session_file_path(&id);
            if !path.exists() {
                bail!("agent session {id} does not exist");
            }
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to delete {}", path.display()))?;
            store.append_event("agent_session_deleted", json!({ "id": id }))?;
            if json {
                json_success(
                    "agent_session",
                    Some("delete"),
                    json!({ "deleted": true, "id": id }),
                )?;
            } else {
                println!("deleted {id}");
            }
        }
        AgentCommand::Import { path, json } => {
            let sessions = read_agent_sessions(&path)?;
            let imported = sessions
                .into_iter()
                .map(|session| {
                    store.save_agent_session(&session)?;
                    Ok(session)
                })
                .collect::<Result<Vec<_>>>()?;
            if json {
                json_success("agent_session_list", Some("import"), imported)?;
            } else {
                println!("imported {} agent session(s)", imported.len());
            }
        }
    }
    Ok(())
}

fn load_agent_session(store: &WorkdeckStore, id: &str) -> Result<AgentSession> {
    store
        .load_agent_sessions()?
        .into_iter()
        .find(|session| session.id == id)
        .with_context(|| format!("agent session {id} does not exist"))
}

fn read_agent_sessions(path: &Path) -> Result<Vec<AgentSession>> {
    let extension = path.extension().and_then(|value| value.to_str());
    let sessions = if extension == Some("jsonl") {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mut sessions = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line.with_context(|| format!("failed to read line {}", index + 1))?;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(&line)
                .with_context(|| format!("failed to parse JSONL line {}", index + 1))?;
            sessions.push(agent_session_from_json_value(value).with_context(|| {
                format!("line {} does not contain an agent session", index + 1)
            })?);
        }
        sessions
    } else {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let value: Value = serde_json::from_reader(file)
            .with_context(|| format!("failed to parse JSON {}", path.display()))?;
        match value {
            Value::Array(values) => values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    agent_session_from_json_value(value).with_context(|| {
                        format!("item {} does not contain an agent session", index + 1)
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            value => vec![
                agent_session_from_json_value(value)
                    .with_context(|| "JSON file does not contain an agent session")?,
            ],
        }
    };

    if sessions.is_empty() {
        bail!("no agent sessions found in {}", path.display());
    }
    Ok(sessions)
}

fn agent_session_from_json_value(value: Value) -> Result<AgentSession> {
    if let Some(session) = value.get("session") {
        return serde_json::from_value(session.clone()).map_err(Into::into);
    }
    if let Some(session) = value.pointer("/payload/session") {
        return serde_json::from_value(session.clone()).map_err(Into::into);
    }
    serde_json::from_value(value).map_err(Into::into)
}

fn handle_issue_command(store: &WorkdeckStore, command: IssueCommand) -> Result<()> {
    match command {
        IssueCommand::List {
            status,
            priority,
            project,
            cycle,
            label,
            assignee,
            due_at,
            json,
        } => {
            let issues = filter_issues(
                store.load_issues()?,
                status,
                priority,
                project,
                cycle,
                label,
                assignee,
                due_at,
            )?;
            if json {
                json_success("issue_list", None, issues)?;
            } else if issues.is_empty() {
                println!("no issues");
            } else {
                for issue in issues {
                    println!(
                        "{:<7} {:<12} {:<7} {}",
                        issue.key,
                        issue.status.label(),
                        issue.priority.label(),
                        issue.title
                    );
                }
            }
        }
        IssueCommand::Create {
            title,
            from_json,
            description,
            status,
            priority,
            project,
            cycle,
            assignee,
            due_at,
            label,
            linked_commit,
            linked_file,
            json,
        } => {
            let input = issue_create_input(
                title,
                from_json,
                description,
                status,
                priority,
                project,
                cycle,
                assignee,
                due_at,
                label,
                linked_commit,
                linked_file,
            )?;
            let mut issue = store.create_issue(input.title)?;
            let update = issue_update(
                None,
                input.description,
                input.status,
                input.priority,
                input.project,
                input.cycle,
                input.assignee,
                input.due_at,
                input.labels,
                input.linked_commits,
            )?;
            issue = store.update_issue(&issue.key, update)?;
            for path in input.linked_files {
                issue = store.link_issue_file(&issue.key, &path)?;
            }
            print_issue_action(issue, json, Some("create"))?;
        }
        IssueCommand::Update {
            key,
            title,
            description,
            status,
            priority,
            project,
            cycle,
            assignee,
            due_at,
            label,
            linked_commit,
            json,
        } => {
            let issue = store.update_issue(
                &key,
                issue_update(
                    title,
                    description,
                    status,
                    priority,
                    project,
                    cycle,
                    assignee,
                    due_at,
                    label,
                    linked_commit,
                )?,
            )?;
            print_issue_action(issue, json, Some("update"))?;
        }
        IssueCommand::Link { key, path, json } => {
            let issue = store.link_issue_file(&key, &path)?;
            print_issue_action(issue, json, Some("link-file"))?;
        }
        IssueCommand::LinkFile { key, path, json } => {
            let issue = store.link_issue_file(&key, &path)?;
            print_issue_action(issue, json, Some("link-file"))?;
        }
        IssueCommand::UnlinkFile { key, path, json } => {
            let mut issue = load_issue(store, &key)?;
            issue.linked_files.retain(|linked| linked != &path);
            issue.touch();
            store.save_issue(&issue)?;
            store.append_event("issue_file_unlinked", json!({ "key": key, "path": path }))?;
            print_issue_action(issue, json, Some("unlink-file"))?;
        }
        IssueCommand::LinkCommit { key, sha, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    linked_commits: Some(vec![sha]),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("link-commit"))?;
        }
        IssueCommand::UnlinkCommit { key, sha, json } => {
            let mut issue = load_issue(store, &key)?;
            issue.linked_commits.retain(|linked| linked != &sha);
            issue.touch();
            store.save_issue(&issue)?;
            store.append_event("issue_commit_unlinked", json!({ "key": key, "sha": sha }))?;
            print_issue_action(issue, json, Some("unlink-commit"))?;
        }
        IssueCommand::Close { key, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    status: Some(IssueStatus::Done),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("close"))?;
        }
        IssueCommand::Reopen { key, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    status: Some(IssueStatus::Todo),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("reopen"))?;
        }
        IssueCommand::Move { key, status, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    status: Some(status.parse().map_err(anyhow::Error::msg)?),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("move"))?;
        }
        IssueCommand::Assign {
            key,
            assignee,
            json,
        } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    assignee: Some(assignee),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("assign"))?;
        }
        IssueCommand::Unassign { key, json } => {
            let issue = store.update_issue(
                &key,
                IssueUpdate {
                    assignee: Some(String::new()),
                    ..IssueUpdate::default()
                },
            )?;
            print_issue_action(issue, json, Some("unassign"))?;
        }
        IssueCommand::Label { command } => match command {
            IssueLabelCommand::Add { key, label, json } => {
                let mut issue = load_issue(store, &key)?;
                if !issue.labels.iter().any(|value| value == &label) {
                    issue.labels.push(label);
                    issue.labels.sort();
                    issue.touch();
                    store.save_issue(&issue)?;
                    store.append_event("issue_label_added", json!({ "key": key }))?;
                }
                print_issue_action(issue, json, Some("label-add"))?;
            }
            IssueLabelCommand::Remove { key, label, json } => {
                let mut issue = load_issue(store, &key)?;
                issue.labels.retain(|value| value != &label);
                issue.touch();
                store.save_issue(&issue)?;
                store.append_event("issue_label_removed", json!({ "key": key, "label": label }))?;
                print_issue_action(issue, json, Some("label-remove"))?;
            }
        },
        IssueCommand::Delete { key, yes, json } => {
            if !yes {
                bail!("delete requires --yes");
            }
            let path = store.issue_file_path(&key);
            if !path.exists() {
                bail!("issue {key} does not exist");
            }
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to delete {}", path.display()))?;
            store.append_event("issue_deleted", json!({ "key": key }))?;
            if json {
                json_success(
                    "issue",
                    Some("delete"),
                    json!({ "deleted": true, "key": key }),
                )?;
            } else {
                println!("deleted {key}");
            }
        }
        IssueCommand::Show { key, json } => {
            let issue = store
                .load_issues()?
                .into_iter()
                .find(|issue| issue.key == key)
                .with_context(|| format!("issue {key} does not exist"))?;
            print_issue(issue, json)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn filter_issues(
    issues: Vec<workdeck_cli::store::Issue>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    label: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
) -> Result<Vec<workdeck_cli::store::Issue>> {
    let status = status
        .map(|value| value.parse::<IssueStatus>())
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let priority = priority
        .map(|value| value.parse::<Priority>())
        .transpose()
        .map_err(anyhow::Error::msg)?;
    Ok(issues
        .into_iter()
        .filter(|issue| status.is_none_or(|status| issue.status == status))
        .filter(|issue| priority.is_none_or(|priority| issue.priority == priority))
        .filter(|issue| {
            project
                .as_ref()
                .is_none_or(|project| &issue.project == project)
        })
        .filter(|issue| cycle.as_ref().is_none_or(|cycle| &issue.cycle == cycle))
        .filter(|issue| {
            label
                .as_ref()
                .is_none_or(|label| issue.labels.iter().any(|value| value == label))
        })
        .filter(|issue| {
            assignee
                .as_ref()
                .is_none_or(|assignee| &issue.assignee == assignee)
        })
        .filter(|issue| due_at.as_ref().is_none_or(|due_at| &issue.due_at == due_at))
        .collect())
}

fn load_issue(store: &WorkdeckStore, key: &str) -> Result<workdeck_cli::store::Issue> {
    store
        .load_issues()?
        .into_iter()
        .find(|issue| issue.key == key)
        .with_context(|| format!("issue {key} does not exist"))
}

#[derive(Debug)]
struct IssueCreateInput {
    title: String,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
    labels: Vec<String>,
    linked_commits: Vec<String>,
    linked_files: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn issue_create_input(
    title: Option<String>,
    from_json: Option<PathBuf>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
    labels: Vec<String>,
    linked_commits: Vec<String>,
    linked_files: Vec<String>,
) -> Result<IssueCreateInput> {
    let mut input = if let Some(path) = from_json {
        issue_create_input_from_json(&path)?
    } else {
        IssueCreateInput {
            title: title
                .clone()
                .with_context(|| "issue title is required unless --from-json is used")?,
            description: None,
            status: None,
            priority: None,
            project: None,
            cycle: None,
            assignee: None,
            due_at: None,
            labels: Vec::new(),
            linked_commits: Vec::new(),
            linked_files: Vec::new(),
        }
    };

    if let Some(title) = title {
        input.title = title;
    }
    input.description = description.or(input.description);
    input.status = status.or(input.status);
    input.priority = priority.or(input.priority);
    input.project = project.or(input.project);
    input.cycle = cycle.or(input.cycle);
    input.assignee = assignee.or(input.assignee);
    input.due_at = due_at.or(input.due_at);
    if !labels.is_empty() {
        input.labels = labels;
    }
    if !linked_commits.is_empty() {
        input.linked_commits = linked_commits;
    }
    if !linked_files.is_empty() {
        input.linked_files = linked_files;
    }
    Ok(input)
}

fn issue_create_input_from_json(path: &Path) -> Result<IssueCreateInput> {
    let mut raw = String::new();
    if path == Path::new("-") {
        std::io::stdin()
            .read_to_string(&mut raw)
            .with_context(|| "failed to read issue JSON from stdin")?;
    } else {
        raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
    }
    let value: Value = serde_json::from_str(&raw).with_context(|| "failed to parse issue JSON")?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .with_context(|| "issue JSON requires title")?
        .to_string();
    Ok(IssueCreateInput {
        title,
        description: json_string(&value, "description"),
        status: json_string(&value, "status"),
        priority: json_string(&value, "priority"),
        project: json_string(&value, "project"),
        cycle: json_string(&value, "cycle"),
        assignee: json_string(&value, "assignee"),
        due_at: json_string(&value, "due_at"),
        labels: json_string_array(&value, "labels"),
        linked_commits: json_string_array(&value, "linked_commits"),
        linked_files: json_string_array(&value, "linked_files"),
    })
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn json_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn issue_update(
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    project: Option<String>,
    cycle: Option<String>,
    assignee: Option<String>,
    due_at: Option<String>,
    labels: Vec<String>,
    linked_commits: Vec<String>,
) -> Result<IssueUpdate> {
    Ok(IssueUpdate {
        title,
        description,
        status: status
            .map(|value| value.parse())
            .transpose()
            .map_err(anyhow::Error::msg)?,
        priority: priority
            .map(|value| value.parse::<Priority>())
            .transpose()
            .map_err(anyhow::Error::msg)?,
        project,
        cycle,
        assignee,
        due_at,
        labels: (!labels.is_empty()).then_some(labels),
        linked_commits: (!linked_commits.is_empty()).then_some(linked_commits),
    })
}

fn print_issue(issue: workdeck_cli::store::Issue, json: bool) -> Result<()> {
    print_issue_action(issue, json, None)
}

fn print_issue_action(
    issue: workdeck_cli::store::Issue,
    json: bool,
    action: Option<&str>,
) -> Result<()> {
    if json {
        json_success("issue", action, issue)?;
    } else {
        println!("{} {}", issue.key, issue.title);
    }
    Ok(())
}

fn print_agent_session(session: AgentSession, json: bool) -> Result<()> {
    print_agent_session_action(session, json, None)
}

fn print_agent_session_action(
    session: AgentSession,
    json: bool,
    action: Option<&str>,
) -> Result<()> {
    if json {
        json_success("agent_session", action, session)?;
    } else {
        println!("{} {}", session.id, session.title);
    }
    Ok(())
}
