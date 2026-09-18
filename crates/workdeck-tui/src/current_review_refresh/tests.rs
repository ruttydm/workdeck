use super::*;
use workdeck_core::{CommonOptions, FileCommandInput, PatchCommandInput, VcsDiffCommandInput};

fn current_view() -> CurrentReviewViewOptions {
    CurrentReviewViewOptions {
        layout_mode: InputLayoutMode::Split,
        theme_id: "nord".into(),
        show_agent_notes: false,
        show_hunk_headers: false,
        show_line_numbers: false,
        show_menu_bar: false,
        wrap_lines: true,
    }
}

fn file_input(options: CommonOptions) -> CliInput {
    CliInput::Files(FileCommandInput {
        left: "before.ts".into(),
        right: "after.ts".into(),
        options,
    })
}

#[test]
fn applies_every_current_view_option_and_retains_unrelated_input_options() {
    let input = file_input(CommonOptions {
        mode: Some(InputLayoutMode::Stack),
        theme: Some("dracula".into()),
        watch: Some(true),
        tab_width: Some(8),
        ..CommonOptions::default()
    });

    let refreshed = with_current_review_view_options(&input, &current_view());
    assert_eq!(refreshed.options().mode, Some(InputLayoutMode::Split));
    assert_eq!(refreshed.options().theme.as_deref(), Some("nord"));
    assert_eq!(refreshed.options().agent_notes, Some(false));
    assert_eq!(refreshed.options().hunk_headers, Some(false));
    assert_eq!(refreshed.options().line_numbers, Some(false));
    assert_eq!(refreshed.options().menu_bar, Some(false));
    assert_eq!(refreshed.options().wrap_lines, Some(true));
    assert_eq!(refreshed.options().watch, Some(true));
    assert_eq!(refreshed.options().tab_width, Some(8));

    assert_eq!(input.options().mode, Some(InputLayoutMode::Stack));
    assert_eq!(input.options().theme.as_deref(), Some("dracula"));
    assert_eq!(input.options().watch, Some(true));
    assert_eq!(input.options().tab_width, Some(8));
}

#[test]
fn attaches_the_source_path_only_to_vcs_inputs() {
    let file_request = derive_workspace_refresh_request(
        &file_input(CommonOptions::default()),
        "/repo",
        &current_view(),
    )
    .expect("file pairs are reloadable");
    let vcs_request = derive_workspace_refresh_request(
        &CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions::default(),
        }),
        "/repo",
        &current_view(),
    )
    .expect("VCS inputs are reloadable");

    assert_eq!(file_request.source_path, None);
    assert_eq!(vcs_request.source_path.as_deref(), Some("/repo"));
}

#[test]
fn stdin_backed_content_and_sidecars_do_not_create_a_descriptor() {
    let stdin_patch = CliInput::Patch(PatchCommandInput {
        file: None,
        text: Some("diff --git a/a b/a".into()),
        options: CommonOptions::default(),
    });
    assert!(derive_workspace_refresh_request(&stdin_patch, "stdin", &current_view()).is_none());

    let input_with_stdin_sidecar = file_input(CommonOptions {
        agent_context: Some("-".into()),
        ..CommonOptions::default()
    });
    assert!(
        derive_workspace_refresh_request(&input_with_stdin_sidecar, "/repo", &current_view())
            .is_none()
    );
}

#[test]
fn named_patch_files_remain_reloadable_without_a_vcs_source_path() {
    let input = CliInput::Patch(PatchCommandInput {
        file: Some("review.patch".into()),
        text: None,
        options: CommonOptions::default(),
    });
    let request = derive_workspace_refresh_request(&input, "review.patch", &current_view())
        .expect("named patch files are reloadable");
    assert_eq!(request.source_path, None);
}
