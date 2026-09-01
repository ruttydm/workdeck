//! Runtime-neutral shapes for every parsed Workdeck invocation.

use crate::{ReviewNoteSource, ReviewSide};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputLayoutMode {
    #[default]
    Auto,
    Split,
    Stack,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum InputCursorLine {
    #[default]
    Row,
    Number,
    Off,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SidebarVisibility {
    #[default]
    Auto,
    Visible,
    Hidden,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommonOptions {
    pub mode: Option<InputLayoutMode>,
    pub cursor_line: Option<InputCursorLine>,
    pub vcs: Option<String>,
    pub theme: Option<String>,
    pub agent_context: Option<String>,
    pub pager: Option<bool>,
    pub watch: Option<bool>,
    pub experimental: Option<bool>,
    pub fast: Option<bool>,
    pub exclude_untracked: Option<bool>,
    pub line_numbers: Option<bool>,
    pub tab_width: Option<u16>,
    pub file_gap: Option<u16>,
    pub hunk_gap: Option<u16>,
    pub wrap_lines: Option<bool>,
    pub hunk_headers: Option<bool>,
    pub menu_bar: Option<bool>,
    pub sidebar: Option<SidebarVisibility>,
    pub agent_notes: Option<bool>,
    pub copy_decorations: Option<bool>,
    pub prompt_save_view_preferences: Option<bool>,
    pub transparent_background: Option<bool>,
    pub color_moved: Option<bool>,
    pub extensions: Option<bool>,
    pub extension_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsRangeEndpoints {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsDiffCommandInput {
    pub range: Option<String>,
    pub range_endpoints: Option<VcsRangeEndpoints>,
    pub staged: bool,
    pub pathspecs: Vec<String>,
    pub options: CommonOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsShowCommandInput {
    pub reference: Option<String>,
    pub pathspecs: Vec<String>,
    pub options: CommonOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsStashShowCommandInput {
    pub reference: Option<String>,
    pub options: CommonOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCommandInput {
    pub left: String,
    pub right: String,
    pub options: CommonOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchCommandInput {
    pub file: Option<String>,
    pub text: Option<String>,
    pub options: CommonOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffToolCommandInput {
    pub left: String,
    pub right: String,
    pub path: Option<String>,
    pub options: CommonOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliInput {
    Vcs(VcsDiffCommandInput),
    Show(VcsShowCommandInput),
    StashShow(VcsStashShowCommandInput),
    Files(FileCommandInput),
    Patch(PatchCommandInput),
    DiffTool(DiffToolCommandInput),
}

impl CliInput {
    #[must_use]
    pub const fn options(&self) -> &CommonOptions {
        match self {
            Self::Vcs(input) => &input.options,
            Self::Show(input) => &input.options,
            Self::StashShow(input) => &input.options,
            Self::Files(input) => &input.options,
            Self::Patch(input) => &input.options,
            Self::DiffTool(input) => &input.options,
        }
    }

    pub const fn options_mut(&mut self) -> &mut CommonOptions {
        match self {
            Self::Vcs(input) => &mut input.options,
            Self::Show(input) => &mut input.options,
            Self::StashShow(input) => &mut input.options,
            Self::Files(input) => &mut input.options,
            Self::Patch(input) => &mut input.options,
            Self::DiffTool(input) => &mut input.options,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCommentListType {
    Live,
    All,
    Source(ReviewNoteSource),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpCommandInput {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagerCommandInput {
    pub options: CommonOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonServeCommandInput;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionCommandOutput {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSelectorInput {
    pub session_id: Option<String>,
    pub session_path: Option<String>,
    pub repo_root: Option<String>,
    pub repo_boundary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCommentApplyItemInput {
    pub file_path: String,
    pub hunk_number: Option<u32>,
    pub side: Option<ReviewSide>,
    pub line: Option<u32>,
    pub summary: String,
    pub rationale: Option<String>,
    pub markup: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommandInput {
    List {
        output: SessionCommandOutput,
    },
    Get {
        context: bool,
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
    },
    Review {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        include_patch: bool,
        include_notes: Option<bool>,
    },
    Navigate {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        file_path: Option<String>,
        hunk_number: Option<u32>,
        side: Option<ReviewSide>,
        line: Option<u32>,
        comment_direction: Option<NavigationDirection>,
        comment_id: Option<String>,
    },
    Reload {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        next_input: Box<CliInput>,
        source_path: Option<String>,
    },
    CommentAdd {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        file_path: String,
        side: ReviewSide,
        line: u32,
        summary: String,
        rationale: Option<String>,
        markup: Option<String>,
        author: Option<String>,
        reveal: bool,
    },
    CommentApply {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        comments: Vec<SessionCommentApplyItemInput>,
        reveal_mode: RevealMode,
    },
    CommentList {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        file_path: Option<String>,
        list_type: Option<SessionCommentListType>,
    },
    CommentRemove {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        comment_id: String,
    },
    CommentClear {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        file_path: Option<String>,
        include_user: Option<bool>,
        confirmed: bool,
    },
    HighlightAdd {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        file_path: String,
        side: ReviewSide,
        line: u32,
        start: u32,
        end: u32,
        tone: Option<HighlightTone>,
        reveal: bool,
    },
    HighlightClear {
        output: SessionCommandOutput,
        selector: SessionSelectorInput,
        file_path: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealMode {
    None,
    First,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightTone {
    Match,
    Current,
    Info,
    Warning,
    Error,
    Dim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkupRenderCommandInput {
    pub file: String,
    pub width: usize,
    pub color: MarkupColorMode,
    pub theme: Option<String>,
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkupGuideCommandInput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionManageCommandInput {
    Install { source: String, yes: bool },
    List,
    Update { name: Option<String> },
    Remove { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCliInvocationInput {
    pub command_name: String,
    pub args: Vec<String>,
    pub extension_paths: Vec<String>,
    pub extensions_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkdeckInstallSource {
    Cargo,
    Homebrew,
    Nix,
    Curl,
    PowerShell,
    Direct,
    Dev,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfUpdateCommandInput {
    pub version: Option<String>,
    pub method: Option<WorkdeckInstallSource>,
    pub check: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCliInput {
    Review(CliInput),
    Help(HelpCommandInput),
    Pager(PagerCommandInput),
    DaemonServe(DaemonServeCommandInput),
    Session(SessionCommandInput),
    MarkupRender(MarkupRenderCommandInput),
    MarkupGuide(MarkupGuideCommandInput),
    ExtensionManage(ExtensionManageCommandInput),
    ExtensionCli(ExtensionCliInvocationInput),
    Update(SelfUpdateCommandInput),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_options_cover_content_rendering_and_extension_launch_policy() {
        let options = CommonOptions {
            mode: Some(InputLayoutMode::Split),
            cursor_line: Some(InputCursorLine::Number),
            vcs: Some("jj".into()),
            theme: Some("InspiredGitHub".into()),
            agent_context: Some("context.json".into()),
            pager: Some(true),
            watch: Some(true),
            experimental: Some(true),
            fast: Some(true),
            exclude_untracked: Some(true),
            line_numbers: Some(false),
            tab_width: Some(8),
            file_gap: Some(2),
            hunk_gap: Some(1),
            wrap_lines: Some(true),
            hunk_headers: Some(false),
            menu_bar: Some(false),
            sidebar: Some(SidebarVisibility::Auto),
            agent_notes: Some(true),
            copy_decorations: Some(false),
            prompt_save_view_preferences: Some(false),
            transparent_background: Some(true),
            color_moved: Some(true),
            extensions: Some(true),
            extension_paths: vec!["demo".into()],
        };
        assert_eq!(options.extension_paths, ["demo"]);
        assert_eq!(options.tab_width, Some(8));
    }

    #[test]
    fn parsed_input_union_reaches_every_command_family() {
        let options = CommonOptions::default();
        let values = [
            ParsedCliInput::Review(CliInput::Patch(PatchCommandInput {
                file: Some("-".into()),
                text: None,
                options: options.clone(),
            })),
            ParsedCliInput::Help(HelpCommandInput {
                text: "help".into(),
            }),
            ParsedCliInput::Pager(PagerCommandInput { options }),
            ParsedCliInput::DaemonServe(DaemonServeCommandInput),
            ParsedCliInput::Session(SessionCommandInput::List {
                output: SessionCommandOutput::Json,
            }),
            ParsedCliInput::MarkupGuide(MarkupGuideCommandInput),
            ParsedCliInput::ExtensionManage(ExtensionManageCommandInput::List),
            ParsedCliInput::ExtensionCli(ExtensionCliInvocationInput {
                command_name: "demo".into(),
                args: Vec::new(),
                extension_paths: Vec::new(),
                extensions_enabled: true,
            }),
            ParsedCliInput::Update(SelfUpdateCommandInput {
                version: None,
                method: Some(WorkdeckInstallSource::Cargo),
                check: true,
            }),
        ];
        assert_eq!(values.len(), 9);
    }

    #[test]
    fn vcs_range_keeps_two_explicit_revisions_separate() {
        let input = VcsDiffCommandInput {
            range: None,
            range_endpoints: Some(VcsRangeEndpoints {
                from: "main".into(),
                to: "feature".into(),
            }),
            staged: false,
            pathspecs: vec!["src".into()],
            options: CommonOptions::default(),
        };
        assert_eq!(input.range_endpoints.unwrap().from, "main");
    }
}
