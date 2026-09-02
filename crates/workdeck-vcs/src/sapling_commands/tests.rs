use super::*;
use std::process::Command;
use tempfile::TempDir;
use workdeck_core::{CommonOptions, VcsRangeEndpoints};

fn diff_input() -> VcsDiffCommandInput {
    VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    }
}

fn missing_context() -> SaplingCommandContext {
    SaplingCommandContext {
        cwd: std::env::temp_dir(),
        sl_executable: PathBuf::from("definitely-not-a-real-sl-binary"),
    }
}

fn sl_available() -> bool {
    Command::new("sl")
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn sl(cwd: &Path, arguments: &[&str]) -> String {
    let output = Command::new("sl")
        .args(["--noninteractive", "--color", "never"])
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run Sapling fixture command");
    assert!(
        output.status.success(),
        "sl {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn create_sl_repo() -> TempDir {
    let directory = TempDir::new().unwrap();
    sl(directory.path(), &["init", "--git"]);
    sl(
        directory.path(),
        &[
            "config",
            "--local",
            "ui.username",
            "Test User <test@example.com>",
        ],
    );
    directory
}

#[test]
fn compares_named_revisions_with_one_revision_argument_per_endpoint() {
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "feature".into(),
    });
    assert_eq!(
        build_sl_diff_args(&input).unwrap(),
        ["diff", "--git", "-r", "main", "-r", "feature"]
    );
}

#[test]
fn rejects_each_option_like_or_empty_endpoint_before_commands_or_untracked_probes() {
    for (from, to, fragment) in [
        ("--from-file", "feature", "looks like a Sapling option"),
        ("main", "--to-file", "looks like a Sapling option"),
        ("", "feature", "empty revision"),
        ("main", "", "empty revision"),
    ] {
        let mut input = diff_input();
        input.range_endpoints = Some(VcsRangeEndpoints {
            from: from.into(),
            to: to.into(),
        });
        assert!(
            build_sl_diff_args(&input)
                .unwrap_err()
                .to_string()
                .contains(fragment)
        );
        assert!(
            list_sl_untracked_files(&input, &missing_context(), None)
                .unwrap_err()
                .to_string()
                .contains(fragment)
        );
    }
}

#[test]
fn passes_an_explicit_revset_through_to_revision_argument() {
    let mut input = diff_input();
    input.range = Some(".^::.".into());
    assert_eq!(
        build_sl_diff_args(&input).unwrap(),
        ["diff", "--git", "-r", ".^::."]
    );
}

#[test]
fn discovers_unknown_files_for_single_target_but_not_two_revision_comparisons() {
    let mut input = diff_input();
    input.range_endpoints = Some(VcsRangeEndpoints {
        from: "main".into(),
        to: "feature".into(),
    });
    assert!(
        list_sl_untracked_files(&input, &missing_context(), None)
            .unwrap()
            .is_empty()
    );

    input.range_endpoints = None;
    input.range = Some(".".into());
    assert!(
        list_sl_untracked_files(&input, &missing_context(), None)
            .unwrap_err()
            .to_string()
            .contains("was not found in PATH")
    );
}

#[test]
fn reports_a_friendly_error_when_sl_is_not_installed_or_not_on_path() {
    let input = SaplingBackedInput::Diff(diff_input());
    let error = run_sl_text(&input, &["root".into()], &missing_context()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Sapling is required for `workdeck diff` when `vcs = \"sl\"`, but `definitely-not-a-real-sl-binary` was not found in PATH."
    );
}

#[test]
fn reports_a_friendly_error_outside_a_sl_repository() {
    let input = SaplingBackedInput::Diff(diff_input());
    let translated = translate_sl_exit_failure(&input, "abort: no repository found");
    assert_eq!(
        translated.to_string(),
        "`workdeck diff` must be run inside a Sapling repository when `vcs = \"sl\"`."
    );

    if !sl_available() {
        return;
    }
    let directory = TempDir::new().unwrap();
    let context = SaplingCommandContext {
        cwd: directory.path().into(),
        ..SaplingCommandContext::default()
    };
    let error = run_sl_text(&input, &["root".into()], &context).unwrap_err();
    assert!(error.to_string().contains("Sapling repository"));
}

#[test]
fn reports_a_friendly_error_for_invalid_revsets() {
    let mut diff = diff_input();
    diff.range = Some("missing_revision".into());
    let input = SaplingBackedInput::Diff(diff.clone());
    let translated = translate_sl_exit_failure(&input, "abort: unknown revision missing_revision");
    assert_eq!(
        translated.to_string(),
        "`workdeck diff missing_revision` could not resolve Sapling revset `missing_revision`."
    );

    if !sl_available() {
        return;
    }
    let directory = create_sl_repo();
    let context = SaplingCommandContext {
        cwd: directory.path().into(),
        ..SaplingCommandContext::default()
    };
    let error = run_sl_text(&input, &build_sl_diff_args(&diff).unwrap(), &context).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("could not resolve Sapling revset")
    );
}

#[test]
fn builds_show_and_status_pathspecs_without_reinterpreting_them() {
    let show = VcsShowCommandInput {
        reference: Some(".".into()),
        pathspecs: vec!["--literal-file".into()],
        options: CommonOptions::default(),
    };
    assert_eq!(
        build_sl_show_args(&show),
        ["diff", "--git", "--change", ".", "--", "--literal-file"]
    );
    let mut diff = diff_input();
    diff.pathspecs = vec!["src/lib.rs".into()];
    assert_eq!(
        build_sl_status_args(&diff),
        [
            "status",
            "--unknown",
            "--print0",
            "--root-relative",
            "--",
            "src/lib.rs"
        ]
    );
}
