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
use std::time::{SystemTime, UNIX_EPOCH};
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
    AgentContext, Changeset, ChangesetSource, CliInput, CommonOptions, DiffToolCommandInput,
    InputCursorLine, InputLayoutMode, PatchCommandInput, ReviewSide, SelfUpdateCommandInput,
    SidebarVisibility, StartupNotice, VcsDiffCommandInput, VcsRangeEndpoints, VcsShowCommandInput,
    VcsStashShowCommandInput, resolve_app_state_path,
};
use workdeck_diff::{
    LanguageMatcher, LanguageRegistration, LanguageRegistry, sanitize_terminal_line,
};
use workdeck_extension_api::{
    CliCommandResult, ExtensionManifest, ExtensionNotificationHub, ExtensionNotifyType,
    FileLanguageGlobTarget, FileLanguageMatcher, Registration,
};
use workdeck_extension_host::{
    LoadStartupExtensionsOptions, LoadedExtension, TrustDecision, TrustStore,
    create_extension_apply_notices, create_extension_load_notices, discover_manifests_with_config,
    load_startup_extensions, resolve_loaded_extension_registrations, resolved_native_vcs_adapters,
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
    ReviewOptions, UserKeyBindingEntry,
};
use workdeck_vcs::{
    AnyProvider, GitProvider, ProviderPreference, VcsAdapter, VcsCatalog, VcsLoadContext,
    VcsReviewInput, bundled_vcs_catalog, detect_vcs, extend_vcs_catalog,
    find_project_root_candidate, get_default_vcs_adapter, get_vcs_adapter, load_vcs_review,
    materialize_vcs_patch_result, operation_from_input, parse_patch_input,
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
    extension: Vec<PathBuf>,
    #[arg(skip)]
    no_extensions: bool,
    #[arg(skip)]
    color_moved: Option<bool>,
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
            sidebar: config.review.sidebar.is_visible(),
            no_sidebar: !config.review.sidebar.is_visible(),
            agent_notes: config.review.agent_notes,
            no_agent_notes: !config.review.agent_notes,
            file_gap: Some(config.review.file_gap),
            hunk_gap: Some(config.review.hunk_gap),
            transparent_background: config.review.transparent_background,
            opaque_background: !config.review.transparent_background,
            agent_context: None,
            theme: (config.ui.theme != "auto").then(|| config.ui.theme.clone()),
            extension: Vec::new(),
            no_extensions: !config.resolved_extensions.enabled,
            color_moved: config.review.color_moved,
            keybindings: config.keybindings.clone(),
            keybinding_notices: config.keybinding_notices.clone(),
            startup_notices: config.startup_notices.clone(),
            extension_config: config.extension_configs().clone(),
            user_extension_paths: config.resolved_extensions.paths.clone(),
            repo_extension_paths: config.resolved_extensions.repo_paths.clone(),
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
            self.sidebar = configured.sidebar;
            self.no_sidebar = configured.no_sidebar;
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
        self.keybindings = configured.keybindings;
        self.keybinding_notices = configured.keybinding_notices;
        self.startup_notices = configured.startup_notices;
        self.extension_config = configured.extension_config;
        self.user_extension_paths = configured.user_extension_paths;
        self.repo_extension_paths = configured.repo_extension_paths;
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
            sidebar: self.sidebar || !self.no_sidebar,
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
            sidebar: Some(if self.sidebar || !self.no_sidebar {
                SidebarVisibility::Visible
            } else {
                SidebarVisibility::Hidden
            }),
            agent_notes: Some(self.agent_notes && !self.no_agent_notes),
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
}

#[cfg(test)]
mod review_cli_option_tests {
    use super::*;

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

fn run(mut args: Args) -> Result<()> {
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
    let prepared_piped_input = prepare_piped_review_input(args.command.as_ref())?;

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
        let preference = command
            .review_options()
            .expect("review command has review options")
            .preference();
        let config_root = AnyProvider::discover(&args.cwd, preference)
            .ok()
            .map(|provider| provider.root().to_owned())
            .unwrap_or_else(|| args.cwd.clone());
        let config = Config::load(&config_root)?;
        command
            .review_options_mut()
            .expect("review command has review options")
            .apply_config_defaults(&config);
        if let Command::Diff {
            exclude_untracked,
            include_untracked,
            ..
        } = &mut command
            && !*exclude_untracked
            && !*include_untracked
        {
            *exclude_untracked = config.review.exclude_untracked;
        }
        return handle_review_command(&args.cwd, command, prepared_piped_input);
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
    let mut review = ReviewCliOptions::from_config(&config);
    review.extension = args.extension;
    review.no_extensions |= args.no_extensions && !args.extensions;
    let mut prepared_extensions = prepare_review_extensions(&args.cwd, &review)?;
    let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
    prepared_extensions.vcs_catalog = Some(catalog.clone());
    let mut vcs_input = VcsReviewInput::Diff(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: {
            let mut options = review.common_options();
            options.exclude_untracked = Some(config.review.exclude_untracked);
            options
        },
    });
    let selection = select_review_vcs_adapter(&args.cwd, review.configured_vcs_id(), &catalog)?;
    if let Some(notice) = selection.unknown_id_notice {
        prepared_extensions.startup_notices.push(notice);
    }
    let adapter = selection.adapter;
    vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
    let loaded = load_selected_vcs_changeset(&args.cwd, &adapter, &catalog, &vcs_input)?;
    if !loaded.changeset.is_empty() {
        let session_input = match &vcs_input {
            VcsReviewInput::Diff(input) => CliInput::Vcs(input.clone()),
            _ => unreachable!(),
        };
        let mut reload = || {
            load_selected_vcs_changeset(&args.cwd, &adapter, &catalog, &vcs_input)
                .map(|loaded| loaded.changeset)
        };
        run_review_with_preloaded_extensions(
            &args.cwd,
            Some(loaded.repo_root),
            loaded.changeset,
            review,
            Some(session_input),
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
    _terminal: Option<workdeck_tui::ControllingTerminal<File>>,
}

struct PreparedReviewExtensions {
    extensions: Vec<LoadedExtension>,
    notifications: ExtensionNotificationHub,
    pending_trust_repo_root: Option<PathBuf>,
    startup_notices: Vec<StartupNotice>,
    vcs_catalog: Option<VcsCatalog>,
}

struct SelectedVcsAdapter {
    adapter: VcsAdapter,
    unknown_id_notice: Option<StartupNotice>,
}

struct LoadedVcsChangeset {
    changeset: Changeset,
    repo_root: PathBuf,
}

fn prepare_review_extensions(
    cwd: &Path,
    review: &ReviewCliOptions,
) -> Result<PreparedReviewExtensions> {
    load_review_extensions(cwd, review)
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
    if !reads_stdin || std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("failed to read piped review input")?;
    let needs_terminal = matches!(command, Some(Command::Patch { .. }))
        || matches!(command, Some(Command::Pager { .. }))
            && workdeck_cli::pager::looks_like_patch_input(&text);
    let terminal = needs_terminal
        .then(attach_controlling_terminal_input)
        .transpose()?;
    Ok(Some(PreparedPipedInput {
        text,
        _terminal: terminal,
    }))
}

fn handle_review_command(
    cwd: &Path,
    command: Command,
    mut prepared_piped_input: Option<PreparedPipedInput>,
) -> Result<()> {
    match command {
        Command::Diff {
            revisions,
            staged,
            exclude_untracked,
            include_untracked: _,
            pathspec,
            review,
        } => {
            let (from, target) = match revisions.as_slice() {
                [] => (None, None),
                [target] => (None, Some(target.clone())),
                [from, to] => (Some(from.clone()), Some(to.clone())),
                _ => unreachable!("clap limits revisions to two"),
            };
            let mut prepared_extensions = prepare_review_extensions(cwd, &review)?;
            let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
            prepared_extensions.vcs_catalog = Some(catalog.clone());
            let mut input_options = review.common_options();
            input_options.exclude_untracked = Some(exclude_untracked);
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
                prepared_extensions.startup_notices.push(notice);
            }
            let adapter = selection.adapter;
            vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
            let loaded = load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)?;
            let session_input = match &vcs_input {
                VcsReviewInput::Diff(input) => CliInput::Vcs(input.clone()),
                _ => unreachable!(),
            };
            let mut reload = || {
                load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)
                    .map(|loaded| loaded.changeset)
            };
            run_review_with_preloaded_extensions(
                cwd,
                Some(loaded.repo_root),
                loaded.changeset,
                review,
                Some(session_input),
                Some(&mut reload),
                prepared_extensions,
            )
        }
        Command::Show {
            target,
            pathspec,
            review,
        } => {
            let mut prepared_extensions = prepare_review_extensions(cwd, &review)?;
            let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
            prepared_extensions.vcs_catalog = Some(catalog.clone());
            let mut vcs_input = VcsReviewInput::Show(VcsShowCommandInput {
                reference: target.clone(),
                pathspecs: pathspec.clone(),
                options: review.common_options(),
            });
            let selection = select_review_vcs_adapter(cwd, review.configured_vcs_id(), &catalog)?;
            if let Some(notice) = selection.unknown_id_notice {
                prepared_extensions.startup_notices.push(notice);
            }
            let adapter = selection.adapter;
            vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
            let loaded = load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)?;
            let session_input = match &vcs_input {
                VcsReviewInput::Show(input) => CliInput::Show(input.clone()),
                _ => unreachable!(),
            };
            let mut reload = || {
                load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)
                    .map(|loaded| loaded.changeset)
            };
            run_review_with_preloaded_extensions(
                cwd,
                Some(loaded.repo_root),
                loaded.changeset,
                review,
                Some(session_input),
                Some(&mut reload),
                prepared_extensions,
            )
        }
        Command::Stash {
            command: StashCommand::Show { reference, review },
        } => {
            let mut prepared_extensions = prepare_review_extensions(cwd, &review)?;
            let catalog = compose_review_vcs_catalog(&prepared_extensions.extensions);
            prepared_extensions.vcs_catalog = Some(catalog.clone());
            let configured_id = review.configured_vcs_id().or(Some("git"));
            let mut vcs_input = VcsReviewInput::StashShow(VcsStashShowCommandInput {
                reference: reference.clone(),
                options: review.common_options(),
            });
            let selection = select_review_vcs_adapter(cwd, configured_id, &catalog)?;
            if let Some(notice) = selection.unknown_id_notice {
                prepared_extensions.startup_notices.push(notice);
            }
            let adapter = selection.adapter;
            vcs_input_options_mut(&mut vcs_input).vcs = Some(adapter.id.clone());
            let loaded = load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)?;
            let session_input = match &vcs_input {
                VcsReviewInput::StashShow(input) => CliInput::StashShow(input.clone()),
                _ => unreachable!(),
            };
            let mut reload = || {
                load_selected_vcs_changeset(cwd, &adapter, &catalog, &vcs_input)
                    .map(|loaded| loaded.changeset)
            };
            run_review_with_preloaded_extensions(
                cwd,
                Some(loaded.repo_root),
                loaded.changeset,
                review,
                Some(session_input),
                Some(&mut reload),
                prepared_extensions,
            )
        }
        Command::Patch { file, review } => {
            let reload_path = file.clone().filter(|path| path != Path::new("-"));
            let reload_input = reload_path.as_ref().map(|path| {
                CliInput::Patch(PatchCommandInput {
                    file: Some(path.to_string_lossy().into_owned()),
                    text: None,
                    options: review.common_options(),
                })
            });
            let (patch, label) = match file {
                Some(path) if path != Path::new("-") => {
                    let patch = std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read patch {}", path.display()))?;
                    (patch, path.display().to_string())
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
                    (patch, "stdin patch".to_owned())
                }
            };
            let changeset = parse_patch_input(&patch, label).map_err(anyhow::Error::from)?;
            if let Some(path) = reload_path {
                let mut reload = || {
                    let patch = std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read patch {}", path.display()))?;
                    parse_patch_input(&patch, path.display().to_string())
                        .map_err(anyhow::Error::from)
                };
                run_review_with_options(cwd, changeset, review, reload_input, Some(&mut reload))
            } else {
                run_review_with_options(cwd, changeset, review, None, None)
            }
        }
        Command::Difftool {
            left,
            right,
            path,
            review,
        } => {
            let input = CliInput::DiffTool(DiffToolCommandInput {
                left: left.to_string_lossy().into_owned(),
                right: right.to_string_lossy().into_owned(),
                path: path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                options: review.common_options(),
            });
            let provider = GitProvider::discover(cwd).map_err(anyhow::Error::from)?;
            let mut changeset = provider.files(&left, &right).map_err(anyhow::Error::from)?;
            if let (Some(path), Some(file)) = (path.as_ref(), changeset.files.first_mut()) {
                file.path = path.to_string_lossy().into_owned();
                changeset.refresh_review_identities();
            }
            let display_path = path.map(|path| path.to_string_lossy().into_owned());
            let mut reload = || {
                let mut changeset = provider.files(&left, &right).map_err(anyhow::Error::from)?;
                if let (Some(path), Some(file)) = (&display_path, changeset.files.first_mut()) {
                    file.path.clone_from(path);
                    changeset.refresh_review_identities();
                }
                Ok(changeset)
            };
            run_review_with_options(cwd, changeset, review, Some(input), Some(&mut reload))
        }
        Command::Pager { review } => {
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
                let changeset = parse_patch_input(&input, "pager").map_err(anyhow::Error::from)?;
                run_review_with_options(cwd, changeset, review, None, None)
            } else {
                let context = workdeck_cli::pager::PlainTextPagerContext::current();
                workdeck_cli::pager::page_plain_text(&input, &context).map_err(anyhow::Error::from)
            }
        }
        _ => unreachable!("non-review command passed to review handler"),
    }
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

fn run_review_with_options(
    cwd: &Path,
    changeset: Changeset,
    review: ReviewCliOptions,
    input: Option<CliInput>,
    reloader: Option<&mut dyn FnMut() -> Result<Changeset>>,
) -> Result<()> {
    let prepared_extensions = prepare_review_extensions(cwd, &review)?;
    run_review_with_preloaded_extensions(
        cwd,
        None,
        changeset,
        review,
        input,
        reloader,
        prepared_extensions,
    )
}

fn run_review_with_preloaded_extensions(
    cwd: &Path,
    vcs_repo_root: Option<PathBuf>,
    mut changeset: Changeset,
    review: ReviewCliOptions,
    input: Option<CliInput>,
    reloader: Option<&mut dyn FnMut() -> Result<Changeset>>,
    prepared_extensions: PreparedReviewExtensions,
) -> Result<()> {
    if review.watch && !review.no_watch && reloader.is_none() {
        bail!("--watch requires a file- or VCS-backed review input");
    }
    apply_agent_context(cwd, review.agent_context.as_deref(), &mut changeset)?;
    let PreparedReviewExtensions {
        mut extensions,
        notifications,
        pending_trust_repo_root,
        startup_notices,
        vcs_catalog,
    } = prepared_extensions;
    changeset = apply_review_extensions(changeset, &mut extensions)?;
    let mut options = review.tui_options();
    options.extension_notifications = Some(notifications.clone());
    options.startup_notices = startup_notices;
    options.pending_extension_trust_repo_root = pending_trust_repo_root;
    options.extension_trust_handler =
        Some(review_extension_trust_handler(cwd, &review, &notifications));
    options.command_cwd = Some(cwd.to_owned());
    options.repo = Some(resolve_review_repo_root(
        cwd,
        review.preference(),
        vcs_repo_root.as_deref(),
    ));
    if let Some(reloader) = reloader {
        let agent_context = review.agent_context.clone();
        let mut reload_extensions = extensions.clone();
        let mut decorated_reload = || {
            let mut changeset = reloader()?;
            apply_agent_context(cwd, agent_context.as_deref(), &mut changeset)?;
            apply_review_extensions(changeset, &mut reload_extensions)
        };
        if let Some(input) = input {
            match vcs_catalog {
                None => workdeck_tui::run_review_with_extensions_input_reload(
                    changeset,
                    options,
                    extensions,
                    input,
                    cwd.to_owned(),
                    &mut decorated_reload,
                ),
                Some(catalog) => workdeck_tui::run_review_with_extensions_catalog_input_reload(
                    changeset,
                    options,
                    extensions,
                    input,
                    cwd.to_owned(),
                    catalog,
                    &mut decorated_reload,
                ),
            }
        } else {
            workdeck_tui::run_review_with_extensions_reload(
                changeset,
                options,
                extensions,
                &mut decorated_reload,
            )
        }
    } else {
        workdeck_tui::run_review_with_extensions(changeset, options, extensions)
    }
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
    review: &ReviewCliOptions,
) -> Result<PreparedReviewExtensions> {
    let notifications = ExtensionNotificationHub::new();
    let (extensions, pending_trust_repo_root, startup_notices) =
        load_review_extensions_with_notifications(cwd, review, &notifications)?;
    Ok(PreparedReviewExtensions {
        extensions,
        notifications,
        pending_trust_repo_root,
        startup_notices,
        vcs_catalog: None,
    })
}

fn load_review_extensions_with_notifications(
    cwd: &Path,
    review: &ReviewCliOptions,
    notifications: &ExtensionNotificationHub,
) -> Result<(Vec<LoadedExtension>, Option<PathBuf>, Vec<StartupNotice>)> {
    if review.no_extensions {
        return Ok((Vec::new(), None, review.startup_notices.clone()));
    }
    let config = user_config_root().map(|root| root.join("workdeck"));
    let trust = load_extension_trust_store();
    let global_extensions = config.as_ref().map(|config| config.join("extensions"));
    let repo = AnyProvider::discover(cwd, review.preference())
        .ok()
        .map(|provider| provider.root().to_owned())
        .or_else(|| find_project_root_candidate(cwd));
    let result = load_startup_extensions(LoadStartupExtensionsOptions {
        enabled: true,
        cwd,
        global_directory: global_extensions.as_deref(),
        repo_root: repo.as_deref(),
        trust: &trust,
        explicit_paths: &review.extension,
        user_config_paths: &review.user_extension_paths,
        repo_config_paths: &review.repo_extension_paths,
        host_version: env!("CARGO_PKG_VERSION"),
        extension_configs: &review.extension_config,
        notifications: Some(notifications.clone()),
        previous_load: None,
    })?;
    let mut startup_notices = review.startup_notices.clone();
    startup_notices.extend(create_extension_load_notices(&result.issues));
    let resolution =
        resolve_loaded_extension_registrations(&result.extensions, bundled_vcs_catalog());
    startup_notices.extend(create_extension_apply_notices(&resolution.issues));
    Ok((
        result.extensions,
        result.pending_trust_repo_root,
        startup_notices,
    ))
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
        let (extensions, pending, notices) =
            load_review_extensions_with_notifications(&cwd, &review, &notifications)
                .map_err(|_| ExtensionTrustHostError::Reload)?;
        if pending.is_some() {
            return Err(ExtensionTrustHostError::Reload);
        }
        for notice in notices {
            notifications.notify(notice.message, ExtensionNotifyType::Warning);
        }
        Ok(extensions)
    })
}

fn load_cli_extensions(
    cwd: &Path,
    explicit: &[PathBuf],
    disabled: bool,
) -> Result<Vec<LoadedExtension>> {
    if disabled {
        return Ok(Vec::new());
    }
    let config = user_config_root().map(|root| root.join("workdeck"));
    let trust = load_extension_trust_store();
    let global_extensions = config.as_ref().map(|config| config.join("extensions"));
    let repo = AnyProvider::discover(cwd, ProviderPreference::Auto)
        .ok()
        .map(|provider| provider.root().to_owned())
        .or_else(|| find_project_root_candidate(cwd));
    let config = Config::load(repo.as_deref().unwrap_or(cwd))?;
    if !config.resolved_extensions.enabled {
        return Ok(Vec::new());
    }
    let result = load_startup_extensions(LoadStartupExtensionsOptions {
        enabled: true,
        cwd,
        global_directory: global_extensions.as_deref(),
        repo_root: repo.as_deref(),
        trust: &trust,
        explicit_paths: explicit,
        user_config_paths: &config.resolved_extensions.paths,
        repo_config_paths: &config.resolved_extensions.repo_paths,
        host_version: env!("CARGO_PKG_VERSION"),
        extension_configs: config.extension_configs(),
        notifications: None,
        previous_load: None,
    })?;
    for issue in result.issues {
        eprintln!("warning: {issue}");
    }
    Ok(result.extensions)
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
    let mut extensions = load_cli_extensions(cwd, extension_paths, extensions_disabled)?;
    let registered = registered_extension_cli_commands(&extensions);
    let resolved = resolve_extension_cli_commands(&registered);
    for issue in create_extension_cli_collision_issues(&registered, &resolved.collisions) {
        eprintln!("workdeck: warning: {}", issue.message);
    }

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
        bail!(message);
    };

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
        extensions[extension_index].invoke_cli_command_cancellable_with_input(
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
            // Retain the registry until the delegated command returns. Extensions may own
            // reloadable temporary inputs that are retired only during extension shutdown.
            let result = run(delegated);
            drop(extensions);
            result
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
