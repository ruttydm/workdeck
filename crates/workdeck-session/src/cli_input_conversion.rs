//! Lossless conversion between core invocation models and the daemon wire shape.

use thiserror::Error;
use workdeck_core::{
    CliInput, CommonOptions, DiffToolCommandInput, FileCommandInput, InputCursorLine,
    InputLayoutMode, PatchCommandInput, SidebarVisibility, VcsDiffCommandInput, VcsRangeEndpoints,
    VcsShowCommandInput, VcsStashShowCommandInput,
};

use crate::{
    DaemonCliInput, DaemonCommonOptions, DaemonCursorLine, DaemonLayoutMode, DaemonRangeEndpoints,
    DaemonSidebarAuto, DaemonSidebarVisibility,
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DaemonCliInputConversionError {
    #[error("{field} exceeds the supported terminal option range")]
    OptionRange { field: &'static str },
}

fn daemon_options(options: CommonOptions) -> DaemonCommonOptions {
    DaemonCommonOptions {
        mode: options.mode.map(|mode| match mode {
            InputLayoutMode::Auto => DaemonLayoutMode::Auto,
            InputLayoutMode::Split => DaemonLayoutMode::Split,
            InputLayoutMode::Stack => DaemonLayoutMode::Stack,
        }),
        cursor_line: options.cursor_line.map(|cursor| match cursor {
            InputCursorLine::Row => DaemonCursorLine::Row,
            InputCursorLine::Number => DaemonCursorLine::Number,
            InputCursorLine::Off => DaemonCursorLine::Off,
        }),
        vcs: options.vcs,
        theme: options.theme,
        agent_context: options.agent_context,
        pager: options.pager,
        watch: options.watch,
        experimental: options.experimental,
        fast: options.fast,
        exclude_untracked: options.exclude_untracked,
        line_numbers: options.line_numbers,
        tab_width: options.tab_width.map(u64::from),
        file_gap: options.file_gap.map(u64::from),
        hunk_gap: options.hunk_gap.map(u64::from),
        wrap_lines: options.wrap_lines,
        hunk_headers: options.hunk_headers,
        menu_bar: options.menu_bar,
        sidebar: options.sidebar.map(|sidebar| match sidebar {
            SidebarVisibility::Auto => DaemonSidebarVisibility::Auto(DaemonSidebarAuto::Auto),
            SidebarVisibility::Visible => DaemonSidebarVisibility::Visible(true),
            SidebarVisibility::Hidden => DaemonSidebarVisibility::Visible(false),
        }),
        agent_notes: options.agent_notes,
        copy_decorations: options.copy_decorations,
        prompt_save_view_preferences: options.prompt_save_view_preferences,
        transparent_background: options.transparent_background,
        color_moved: options.color_moved,
        extensions: options.extensions,
        extension_paths: (!options.extension_paths.is_empty()).then_some(options.extension_paths),
    }
}

/// Project a provider-neutral invocation onto the authenticated daemon protocol.
#[must_use]
pub fn core_cli_input_to_daemon(input: CliInput) -> DaemonCliInput {
    match input {
        CliInput::Vcs(input) => DaemonCliInput::Vcs {
            range: input.range,
            range_endpoints: input.range_endpoints.map(|range| DaemonRangeEndpoints {
                from: range.from,
                to: range.to,
            }),
            staged: input.staged,
            pathspecs: (!input.pathspecs.is_empty()).then_some(input.pathspecs),
            options: daemon_options(input.options),
        },
        CliInput::Show(input) => DaemonCliInput::Show {
            reference: input.reference,
            pathspecs: (!input.pathspecs.is_empty()).then_some(input.pathspecs),
            options: daemon_options(input.options),
        },
        CliInput::StashShow(input) => DaemonCliInput::StashShow {
            reference: input.reference,
            options: daemon_options(input.options),
        },
        CliInput::Files(input) => DaemonCliInput::Diff {
            left: input.left,
            right: input.right,
            options: daemon_options(input.options),
        },
        CliInput::Patch(input) => DaemonCliInput::Patch {
            file: input.file,
            text: input.text,
            options: daemon_options(input.options),
        },
        CliInput::DiffTool(input) => DaemonCliInput::Difftool {
            left: input.left,
            right: input.right,
            path: input.path,
            options: daemon_options(input.options),
        },
    }
}

fn bounded_u16(
    field: &'static str,
    value: Option<u64>,
) -> Result<Option<u16>, DaemonCliInputConversionError> {
    value
        .map(u16::try_from)
        .transpose()
        .map_err(|_| DaemonCliInputConversionError::OptionRange { field })
}

fn core_options(
    options: DaemonCommonOptions,
) -> Result<CommonOptions, DaemonCliInputConversionError> {
    Ok(CommonOptions {
        mode: options.mode.map(|mode| match mode {
            DaemonLayoutMode::Auto => InputLayoutMode::Auto,
            DaemonLayoutMode::Split => InputLayoutMode::Split,
            DaemonLayoutMode::Stack => InputLayoutMode::Stack,
        }),
        cursor_line: options.cursor_line.map(|cursor| match cursor {
            DaemonCursorLine::Row => InputCursorLine::Row,
            DaemonCursorLine::Number => InputCursorLine::Number,
            DaemonCursorLine::Off => InputCursorLine::Off,
        }),
        vcs: options.vcs,
        theme: options.theme,
        agent_context: options.agent_context,
        pager: options.pager,
        watch: options.watch,
        experimental: options.experimental,
        fast: options.fast,
        exclude_untracked: options.exclude_untracked,
        line_numbers: options.line_numbers,
        tab_width: bounded_u16("tab width", options.tab_width)?,
        file_gap: bounded_u16("file gap", options.file_gap)?,
        hunk_gap: bounded_u16("hunk gap", options.hunk_gap)?,
        wrap_lines: options.wrap_lines,
        hunk_headers: options.hunk_headers,
        menu_bar: options.menu_bar,
        sidebar: options.sidebar.map(|sidebar| match sidebar {
            DaemonSidebarVisibility::Auto(DaemonSidebarAuto::Auto) => SidebarVisibility::Auto,
            DaemonSidebarVisibility::Visible(true) => SidebarVisibility::Visible,
            DaemonSidebarVisibility::Visible(false) => SidebarVisibility::Hidden,
        }),
        agent_notes: options.agent_notes,
        copy_decorations: options.copy_decorations,
        prompt_save_view_preferences: options.prompt_save_view_preferences,
        transparent_background: options.transparent_background,
        color_moved: options.color_moved,
        extensions: options.extensions,
        extension_paths: options.extension_paths.unwrap_or_default(),
    })
}

/// Decode a validated daemon command back into the common core invocation model.
pub fn daemon_cli_input_to_core(
    input: DaemonCliInput,
) -> Result<CliInput, DaemonCliInputConversionError> {
    Ok(match input {
        DaemonCliInput::Vcs {
            range,
            range_endpoints,
            staged,
            pathspecs,
            options,
        } => CliInput::Vcs(VcsDiffCommandInput {
            range,
            range_endpoints: range_endpoints.map(|range| VcsRangeEndpoints {
                from: range.from,
                to: range.to,
            }),
            staged,
            pathspecs: pathspecs.unwrap_or_default(),
            options: core_options(options)?,
        }),
        DaemonCliInput::Show {
            reference,
            pathspecs,
            options,
        } => CliInput::Show(VcsShowCommandInput {
            reference,
            pathspecs: pathspecs.unwrap_or_default(),
            options: core_options(options)?,
        }),
        DaemonCliInput::StashShow { reference, options } => {
            CliInput::StashShow(VcsStashShowCommandInput {
                reference,
                options: core_options(options)?,
            })
        }
        DaemonCliInput::Diff {
            left,
            right,
            options,
        } => CliInput::Files(FileCommandInput {
            left,
            right,
            options: core_options(options)?,
        }),
        DaemonCliInput::Patch {
            file,
            text,
            options,
        } => CliInput::Patch(PatchCommandInput {
            file,
            text,
            options: core_options(options)?,
        }),
        DaemonCliInput::Difftool {
            left,
            right,
            path,
            options,
        } => CliInput::DiffTool(DiffToolCommandInput {
            left,
            right,
            path,
            options: core_options(options)?,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_core_variant_round_trips_without_losing_options() {
        let options = CommonOptions {
            mode: Some(InputLayoutMode::Split),
            cursor_line: Some(InputCursorLine::Number),
            vcs: Some("jj".into()),
            theme: Some("nord".into()),
            agent_context: Some("context.json".into()),
            pager: Some(false),
            watch: Some(true),
            experimental: Some(true),
            fast: Some(false),
            exclude_untracked: Some(true),
            line_numbers: Some(false),
            tab_width: Some(8),
            file_gap: Some(2),
            hunk_gap: Some(1),
            wrap_lines: Some(true),
            hunk_headers: Some(false),
            menu_bar: Some(false),
            sidebar: Some(SidebarVisibility::Visible),
            agent_notes: Some(true),
            copy_decorations: Some(true),
            prompt_save_view_preferences: Some(false),
            transparent_background: Some(true),
            color_moved: Some(false),
            extensions: Some(true),
            extension_paths: vec!["./extension".into()],
        };
        let variants = vec![
            CliInput::Vcs(VcsDiffCommandInput {
                range: Some("HEAD".into()),
                range_endpoints: Some(VcsRangeEndpoints {
                    from: "a".into(),
                    to: "b".into(),
                }),
                staged: true,
                pathspecs: vec!["src".into()],
                options: options.clone(),
            }),
            CliInput::Show(VcsShowCommandInput {
                reference: Some("HEAD~1".into()),
                pathspecs: vec!["README.md".into()],
                options: options.clone(),
            }),
            CliInput::StashShow(VcsStashShowCommandInput {
                reference: Some("stash@{1}".into()),
                options: options.clone(),
            }),
            CliInput::Files(FileCommandInput {
                left: "a".into(),
                right: "b".into(),
                options: options.clone(),
            }),
            CliInput::Patch(PatchCommandInput {
                file: Some("review.patch".into()),
                text: None,
                options: options.clone(),
            }),
            CliInput::DiffTool(DiffToolCommandInput {
                left: "old".into(),
                right: "new".into(),
                path: Some("src/lib.rs".into()),
                options,
            }),
        ];
        for input in variants {
            assert_eq!(
                daemon_cli_input_to_core(core_cli_input_to_daemon(input.clone())).unwrap(),
                input
            );
        }
    }

    #[test]
    fn oversized_terminal_dimensions_are_rejected_before_narrowing() {
        let input = DaemonCliInput::Patch {
            file: Some("review.patch".into()),
            text: None,
            options: DaemonCommonOptions {
                tab_width: Some(u64::from(u16::MAX) + 1),
                ..DaemonCommonOptions::default()
            },
        };
        assert_eq!(
            daemon_cli_input_to_core(input).unwrap_err(),
            DaemonCliInputConversionError::OptionRange { field: "tab width" }
        );
    }
}
