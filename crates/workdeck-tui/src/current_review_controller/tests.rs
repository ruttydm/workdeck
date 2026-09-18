use super::*;
use crate::CurrentReviewViewOptions;
use workdeck_core::{
    CliInput, CommonOptions, FileCommandInput, InputLayoutMode, PatchCommandInput,
};

fn view(theme: &str) -> CurrentReviewViewOptions {
    CurrentReviewViewOptions {
        layout_mode: InputLayoutMode::Stack,
        theme_id: theme.into(),
        show_agent_notes: true,
        show_hunk_headers: true,
        show_line_numbers: true,
        show_menu_bar: true,
        wrap_lines: false,
    }
}

fn reloadable_input() -> CliInput {
    CliInput::Files(FileCommandInput {
        left: "before.ts".into(),
        right: "after.ts".into(),
        options: CommonOptions::default(),
    })
}

#[test]
fn replaces_the_registered_descriptor_while_operations_use_the_latest_value() {
    let input = reloadable_input();
    let mut controller = CurrentReviewRefreshController::new(&input, "/repo", &view("dracula"));
    let first = controller.request().expect("registered request").clone();

    controller.update(&input, "/repo", &view("nord"));
    let current = controller.request().expect("replacement request").clone();
    assert_ne!(first, current);
    assert_eq!(current.next_input.options().theme.as_deref(), Some("nord"));

    let mut reloads = Vec::new();
    let called = controller
        .refresh_current_input(
            CurrentReviewRefreshOptions {
                reason: Some(SessionReloadReason::Manual),
                reload_extensions: Some(true),
            },
            &mut |input, options| {
                reloads.push((input.clone(), options.clone()));
                Ok::<(), &'static str>(())
            },
        )
        .expect("reload succeeds");

    assert!(called);
    assert_eq!(reloads.len(), 1);
    assert_eq!(reloads[0].0, current.next_input);
    assert_eq!(reloads[0].1.reason, Some(SessionReloadReason::Manual));
    assert_eq!(reloads[0].1.reload_extensions, Some(true));
    assert!(!reloads[0].1.reset_app);
    assert_eq!(reloads[0].1.source_path, None);
}

#[test]
fn manual_reload_reports_a_rejection_while_the_general_operation_preserves_it() {
    let controller =
        CurrentReviewRefreshController::new(&reloadable_input(), "/repo", &view("dracula"));
    let failure = "reload failed";
    let error = controller
        .refresh_current_input(
            CurrentReviewRefreshOptions {
                reason: Some(SessionReloadReason::Manual),
                reload_extensions: None,
            },
            &mut |_input, _options| Err(failure),
        )
        .expect_err("general operation preserves the failure");
    assert_eq!(error, failure);

    let mut reported = Vec::new();
    let reloaded = controller
        .trigger_refresh_current_input(&mut |_input, _options| Err(failure), &mut |error| {
            reported.push(*error)
        });
    assert!(!reloaded);
    assert_eq!(reported, [failure]);
}

#[test]
fn keeps_manual_and_watch_reasons_distinct() {
    let controller =
        CurrentReviewRefreshController::new(&reloadable_input(), "/repo", &view("dracula"));
    let mut reasons = Vec::new();
    let mut reload = |_input: &CliInput, options: &CurrentReviewReloadOptions| {
        reasons.push(options.reason);
        assert!(!options.reset_app);
        Ok::<(), &'static str>(())
    };
    assert!(controller.trigger_refresh_current_input(&mut reload, &mut |_| {}));
    assert!(
        controller
            .refresh_watched_input(&mut reload)
            .expect("watch refresh succeeds")
    );
    assert_eq!(
        reasons,
        [
            Some(SessionReloadReason::Manual),
            Some(SessionReloadReason::Watch)
        ]
    );
}

#[test]
fn non_reloadable_input_is_unregistered_and_inert() {
    let input = CliInput::Patch(PatchCommandInput {
        file: None,
        text: Some("stdin patch".into()),
        options: CommonOptions {
            watch: Some(true),
            ..CommonOptions::default()
        },
    });
    let controller = CurrentReviewRefreshController::new(&input, "stdin", &view("dracula"));
    assert!(!controller.can_refresh_current_input());
    assert!(controller.request().is_none());

    let mut reloads = 0;
    let mut reload = |_input: &CliInput, _options: &CurrentReviewReloadOptions| {
        reloads += 1;
        Ok::<(), &'static str>(())
    };
    assert!(!controller.trigger_refresh_current_input(&mut reload, &mut |_| {}));
    assert!(
        !controller
            .refresh_watched_input(&mut reload)
            .expect("inert watch refresh succeeds")
    );
    assert_eq!(reloads, 0);
}
