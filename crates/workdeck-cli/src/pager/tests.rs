use super::*;

use std::sync::Mutex;

const OSC52_CLIPBOARD: &str = "\x1b]52;c;SGVsbG8=\x07";
const CSI_CLEAR_SCREEN: &str = "\x1b[2J";
const DCS_PAYLOAD: &str = "\x1bPqpayload\x1b\\";

fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}

fn assert_no_unsafe_controls(text: &str) {
    for control in [
        OSC52_CLIPBOARD,
        CSI_CLEAR_SCREEN,
        DCS_PAYLOAD,
        "\x07",
        "\r",
        "\x08",
    ] {
        assert!(!text.contains(control), "retained {control:?}");
    }
}

struct CapturingRunner {
    calls: Mutex<Vec<(PagerInvocation, String)>>,
    result: Mutex<Result<i32, String>>,
}

impl CapturingRunner {
    fn succeeding() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(Ok(0)),
        })
    }
}

impl PagerRunner for CapturingRunner {
    fn run(&self, invocation: &PagerInvocation, text: &str) -> Result<i32, String> {
        self.calls
            .lock()
            .unwrap()
            .push((invocation.clone(), text.into()));
        self.result.lock().unwrap().clone()
    }
}

fn context(
    environment: BTreeMap<String, String>,
    terminal: bool,
    runner: Arc<dyn PagerRunner>,
) -> PlainTextPagerContext {
    PlainTextPagerContext {
        env: environment,
        stdout_is_terminal: terminal,
        runner,
    }
}

#[test]
fn detects_git_patch_input_even_when_ansi_colored() {
    let patch = [
        "\x1b[1mdiff --git a/src/example.rs b/src/example.rs\x1b[m",
        "index 1111111..2222222 100644",
        "--- a/src/example.rs",
        "+++ b/src/example.rs",
        "@@ -1 +1,2 @@",
        "-let value = 1;",
        "+let value = 2;",
    ]
    .join("\n");
    assert!(looks_like_patch_input(&patch));
}

#[test]
fn detects_common_patch_shapes_across_line_endings_and_terminal_wrappers() {
    let fixtures = [
        vec![
            "diff --git a/example b/example",
            "--- a/example",
            "+++ b/example",
            "@@ -1 +1 @@",
        ],
        vec!["--- a/example", "+++ b/example", "@@ -1 +1 @@"],
        vec!["header", "@@ -10,0 +11,2 @@", "+inserted"],
    ];
    for lines in fixtures {
        for newline in ["\n", "\r\n"] {
            let patch = lines.join(newline);
            assert!(looks_like_patch_input(&patch));
            assert!(looks_like_patch_input(&format!(
                "\x1b]0;title\x07{patch}\x1bPignored\x1b\\"
            )));
        }
    }
}

#[test]
fn partial_markers_and_plain_git_text_are_not_patches() {
    for text in [
        "* main\n  feat/review",
        "--- separator only\nstill prose",
        "+++ banner only\nstill prose",
        "@@section heading\nstill prose",
        "\x1b]0;title\x07--- looks patchy\n+++but is just text",
    ] {
        assert!(!looks_like_patch_input(text), "misclassified {text:?}");
    }
}

#[test]
fn routes_non_diff_input_by_host_capability() {
    let text = "* main\n  feature/demo\n";
    assert_eq!(
        resolve_pager_startup_route(text, &env(&[("TERM", "xterm-256color")]), true, true,),
        PagerStartupRoute::PlainText,
    );
    for marker in [
        ("LV", "-c"),
        ("GIT_PAGER", "workdeck"),
        ("LAZYGIT_NEW_DIR_FILE", "/tmp/dir"),
    ] {
        assert_eq!(
            resolve_pager_startup_route(text, &env(&[("TERM", "dumb"), marker]), true, true,),
            PagerStartupRoute::Passthrough {
                preserve_color: true,
            },
        );
    }
    assert_eq!(
        resolve_pager_startup_route(text, &env(&[("TERM", "dumb")]), true, true),
        PagerStartupRoute::Passthrough {
            preserve_color: false,
        },
    );
}

#[test]
fn routes_diff_input_by_stdout_host_and_controlling_terminal() {
    let patch = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(
        resolve_pager_startup_route(patch, &env(&[("TERM", "xterm-256color")]), true, true,),
        PagerStartupRoute::InteractiveDiff,
    );
    assert_eq!(
        resolve_pager_startup_route(patch, &env(&[("TERM", "xterm-256color")]), false, true,),
        PagerStartupRoute::Passthrough {
            preserve_color: false,
        },
    );
    assert_eq!(
        resolve_pager_startup_route(patch, &env(&[("TERM", "dumb")]), true, true),
        PagerStartupRoute::Passthrough {
            preserve_color: false,
        },
    );
    assert_eq!(
        resolve_pager_startup_route(patch, &env(&[("TERM", "dumb"), ("LV", "-c")]), true, true,),
        PagerStartupRoute::StaticDiff,
    );
    assert_eq!(
        resolve_pager_startup_route(patch, &env(&[("TERM", "xterm-256color")]), true, false,),
        PagerStartupRoute::StaticDiff,
    );
}

#[test]
fn passthrough_preserves_only_safe_sgr_when_requested() {
    let input = format!("\x1b[31mred\x1b[0m{CSI_CLEAR_SCREEN}{OSC52_CLIPBOARD}");
    let mut colored = Vec::new();
    write_passthrough_with(&input, true, &mut colored).unwrap();
    let colored = String::from_utf8(colored).unwrap();
    assert!(colored.contains("\x1b[31mred\x1b[0m"));
    assert!(!colored.contains(CSI_CLEAR_SCREEN));
    assert!(!colored.contains(OSC52_CLIPBOARD));

    let mut plain = Vec::new();
    write_passthrough_with(&input, false, &mut plain).unwrap();
    assert_eq!(String::from_utf8(plain).unwrap(), "red");
}

#[test]
fn falls_back_to_less_when_no_pager_is_configured() {
    assert_eq!(resolve_text_pager_command(&BTreeMap::new()), "less -R");
}

#[test]
fn workdeck_text_pager_wins_and_recursive_workdeck_launches_fall_back() {
    assert_eq!(
        resolve_text_pager_command(&env(&[(TEXT_PAGER_ENV, "bat --paging=always")])),
        "bat --paging=always"
    );
    for recursive in [
        (TEXT_PAGER_ENV, "workdeck pager"),
        ("PAGER", "env FOO=1 workdeck pager"),
        ("PAGER", r#""C:\tools\workdeck.exe" pager"#),
    ] {
        assert_eq!(resolve_text_pager_command(&env(&[recursive])), "less -R");
    }
}

#[test]
fn redirected_stdout_writes_directly_without_spawning() {
    let runner = CapturingRunner::succeeding();
    let mut output = Vec::new();
    page_plain_text_with(
        "plain text output",
        &context(BTreeMap::new(), false, runner.clone()),
        &mut output,
    )
    .unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "plain text output");
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[test]
fn redirected_stdout_strips_terminal_control_sequences() {
    let runner = CapturingRunner::succeeding();
    let mut output = Vec::new();
    let unsafe_text = format!(
        "plain{OSC52_CLIPBOARD}{CSI_CLEAR_SCREEN}{DCS_PAYLOAD}\x07\rspoof\x08hidden\x1b text"
    );
    page_plain_text_with(
        &unsafe_text,
        &context(BTreeMap::new(), false, runner),
        &mut output,
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("plain") && output.contains("spoof") && output.contains("hidden"));
    assert_no_unsafe_controls(&output);
}

#[test]
fn terminal_pager_is_spawned_as_literal_argv_without_a_shell() {
    let runner = CapturingRunner::succeeding();
    page_plain_text_with(
        "needs pager",
        &context(env(&[("PAGER", "less -R")]), true, runner.clone()),
        &mut Vec::new(),
    )
    .unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls[0].0.command, "less");
    assert_eq!(calls[0].0.args, ["-R"]);
    assert_eq!(calls[0].1, "needs pager");
}

#[test]
fn terminal_pager_input_is_sanitized() {
    let runner = CapturingRunner::succeeding();
    let unsafe_text = format!(
        "plain{OSC52_CLIPBOARD}{CSI_CLEAR_SCREEN}{DCS_PAYLOAD}\x07\rspoof\x08hidden\x1b text"
    );
    page_plain_text_with(
        &unsafe_text,
        &context(env(&[("PAGER", "less -R")]), true, runner.clone()),
        &mut Vec::new(),
    )
    .unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_no_unsafe_controls(&calls[0].1);
    assert!(calls[0].1.contains("spoof") && calls[0].1.contains("hidden"));
}

#[test]
fn terminal_pager_preserves_sgr_colors_but_not_screen_control() {
    let runner = CapturingRunner::succeeding();
    let text = format!("* \x1b[1;34mabc1234\x1b[m - \x1b[1;32m(main)\x1b[m{CSI_CLEAR_SCREEN}");
    page_plain_text_with(
        &text,
        &context(env(&[("PAGER", "less -R")]), true, runner.clone()),
        &mut Vec::new(),
    )
    .unwrap();
    let calls = runner.calls.lock().unwrap();
    assert!(calls[0].1.contains("\x1b[1;34mabc1234\x1b[m"));
    assert!(calls[0].1.contains("\x1b[1;32m(main)\x1b[m"));
    assert!(!calls[0].1.contains(CSI_CLEAR_SCREEN));
}

#[test]
fn shell_metacharacters_are_arguments_instead_of_operations() {
    let runner = CapturingRunner::succeeding();
    page_plain_text_with(
        "plain text",
        &context(
            env(&[(
                TEXT_PAGER_ENV,
                "cat >/tmp/workdeck-owned; touch /tmp/workdeck-owned",
            )]),
            true,
            runner.clone(),
        ),
        &mut Vec::new(),
    )
    .unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls[0].0.command, "cat");
    assert_eq!(
        calls[0].0.args,
        [
            ">",
            "/tmp/workdeck-owned",
            ";",
            "touch",
            "/tmp/workdeck-owned"
        ]
    );
}

#[test]
fn quoted_arguments_variables_and_inline_environment_are_preserved() {
    let runner = CapturingRunner::succeeding();
    page_plain_text_with(
        "plain text",
        &context(
            env(&[(
                "PAGER",
                "LESS='-R -F' \"less\" --pattern='hello world' --prompt=$LESS",
            )]),
            true,
            runner.clone(),
        ),
        &mut Vec::new(),
    )
    .unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls[0].0.command, "less");
    assert_eq!(calls[0].0.args, ["--pattern=hello world", "--prompt=$LESS"]);
    assert_eq!(
        calls[0].0.env.get("LESS").map(String::as_str),
        Some("-R -F")
    );
}

#[test]
fn quoted_windows_pager_paths_keep_backslashes() {
    let runner = CapturingRunner::succeeding();
    page_plain_text_with(
        "plain text",
        &context(
            env(&[("PAGER", r#""C:\Program Files\less\less.exe" -R"#)]),
            true,
            runner.clone(),
        ),
        &mut Vec::new(),
    )
    .unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls[0].0.command, r"C:\Program Files\less\less.exe");
    assert_eq!(calls[0].0.args, ["-R"]);
}

#[test]
fn env_wrapper_is_supported_while_recursive_workdeck_is_blocked() {
    assert_eq!(
        resolve_text_pager_command(&env(&[("PAGER", "env LESS=FRX workdeck pager")])),
        "less -R"
    );
    let runner = CapturingRunner::succeeding();
    page_plain_text_with(
        "plain text",
        &context(
            env(&[("PAGER", "env LESS=FRX less -R")]),
            true,
            runner.clone(),
        ),
        &mut Vec::new(),
    )
    .unwrap();
    let calls = runner.calls.lock().unwrap();
    assert_eq!(calls[0].0.command, "less");
    assert_eq!(calls[0].0.env.get("LESS").map(String::as_str), Some("FRX"));
}

#[test]
fn nonzero_pager_status_is_an_error_after_input_delivery() {
    let runner = CapturingRunner::succeeding();
    *runner.result.lock().unwrap() = Ok(1);
    let error = page_plain_text_with(
        "needs pager",
        &context(env(&[("PAGER", "less -R")]), true, runner.clone()),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "Pager command failed: less -R");
    assert_eq!(runner.calls.lock().unwrap()[0].1, "needs pager");
}

#[test]
fn asynchronous_spawn_failure_names_the_selected_pager() {
    let runner = CapturingRunner::succeeding();
    *runner.result.lock().unwrap() = Err("spawn ENOENT".into());
    let error = page_plain_text_with(
        "needs pager",
        &context(env(&[("PAGER", "missing-pager")]), true, runner),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "Pager command failed: missing-pager");
    assert_eq!(error.detail.as_deref(), Some("spawn ENOENT"));
}
