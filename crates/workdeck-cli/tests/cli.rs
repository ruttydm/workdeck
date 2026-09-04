use assert_cmd::prelude::*;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn workdeck() -> Command {
    let mut command = Command::cargo_bin("workdeck").unwrap();
    command.env("HOME", "/nonexistent/workdeck-test-home");
    command
}

fn write_native_extension(directory: &std::path::Path, id: &str) {
    fs::create_dir_all(directory).unwrap();
    fs::write(
        directory.join("workdeck-extension.toml"),
        format!(
            "id = '{id}'\nname = '{id}'\nversion = '1.0.0'\napi_version = 1\nexecutable = '{id}'\ncapabilities = []\n"
        ),
    )
    .unwrap();
}

#[test]
fn help_renders() {
    workdeck()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Terminal-native sidecar"));
}

#[test]
fn version_renders() {
    workdeck()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn daemon_overview_is_headless_and_does_not_require_a_repository() {
    workdeck()
        .args([
            "--cwd",
            "/definitely/missing/workdeck/daemon-root",
            "daemon",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: workdeck daemon serve"))
        .stdout(predicate::str::contains("WORKDECK_MCP_PORT"));

    workdeck()
        .args(["daemon", "serve", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Run the local session daemon and WebSocket broker",
        ));
}

#[test]
fn update_help_lists_only_native_install_channels() {
    workdeck()
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "cargo, brew, nix, curl, powershell, or direct",
        ))
        .stdout(predicate::str::contains("npm").not())
        .stdout(predicate::str::contains("bun").not());
}

#[test]
fn update_rejects_invalid_inputs_before_network_or_repository_access() {
    workdeck()
        .args(["update", "--method", "apt"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Unknown update method: apt"))
        .stderr(predicate::str::contains("Supported methods are"));
    workdeck()
        .args(["update", "latest"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Invalid version: latest"));
}

#[test]
fn update_preserves_managed_channel_exit_semantics() {
    workdeck()
        .args(["update", "--method", "nix"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Workdeck was installed with Nix."))
        .stderr(predicate::str::is_empty());
    workdeck()
        .args(["update", "--method", "nix", "--check"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Update it through your Nix configuration",
        ));
}

#[test]
fn extension_trust_uses_shared_state_preserves_siblings_and_gates_discovery() {
    let repo = tempdir().unwrap();
    let config = tempdir().unwrap();
    let extension = repo
        .path()
        .join(".agents/workdeck/extensions/demo/workdeck-extension.toml");
    fs::create_dir_all(extension.parent().unwrap()).unwrap();
    fs::write(
        &extension,
        "id = 'demo'\nname = 'Demo'\nversion = '0.1.0'\napi_version = 1\nexecutable = 'demo'\ncapabilities = []\n",
    )
    .unwrap();
    let state_path = config.path().join("workdeck/state.json");
    fs::create_dir_all(state_path.parent().unwrap()).unwrap();
    fs::write(
        &state_path,
        r#"{"version":1,"lastSeenCliVersion":"0.17.0"}"#,
    )
    .unwrap();

    let mut trust = workdeck();
    trust
        .env("XDG_CONFIG_HOME", config.path())
        .arg("--cwd")
        .arg(repo.path())
        .args(["extension", "trust", "--allow", "--yes", "--json"])
        .assert()
        .success();

    let state: Value = serde_json::from_str(&fs::read_to_string(&state_path).unwrap()).unwrap();
    assert_eq!(state["lastSeenCliVersion"], "0.17.0");
    let canonical_repo = fs::canonicalize(repo.path()).unwrap();
    assert_eq!(
        state["extensionTrust"][canonical_repo.to_string_lossy().as_ref()],
        "trusted"
    );
    let mut list = workdeck();
    list.env("XDG_CONFIG_HOME", config.path())
        .arg("--cwd")
        .arg(repo.path())
        .args(["extension", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"demo\""));

    let mut deny = workdeck();
    deny.env("XDG_CONFIG_HOME", config.path())
        .arg("--cwd")
        .arg(repo.path())
        .args(["extension", "trust", "--deny", "--yes"])
        .assert()
        .success();
    let mut denied_list = workdeck();
    denied_list
        .env("XDG_CONFIG_HOME", config.path())
        .arg("--cwd")
        .arg(repo.path())
        .args(["extension", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"data\": []"));
}

#[test]
fn extension_discovery_honors_config_provenance_xdg_and_provider_neutral_repo_root() {
    let root = tempdir().unwrap();
    let config = tempdir().unwrap();
    let repo = root.path().join("repo");
    let nested = repo.join("src/nested");
    let user_extension = root.path().join("user-extension");
    let repo_extension = root.path().join("outside-repo-extension");
    let global_extension = config.path().join("workdeck/extensions/global-extension");
    write_native_extension(&user_extension, "user-path");
    write_native_extension(&repo_extension, "repo-path");
    write_native_extension(&global_extension, "global-path");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(repo.join(".agents/workdeck")).unwrap();
    fs::write(
        config.path().join("workdeck/config.toml"),
        format!(
            "[extensions]\npaths = [{}]\n",
            toml::Value::String(user_extension.to_string_lossy().into())
        ),
    )
    .unwrap();
    fs::write(
        repo.join(".agents/workdeck/config.toml"),
        format!(
            "[extensions]\npaths = [{}]\n",
            toml::Value::String(repo_extension.to_string_lossy().into())
        ),
    )
    .unwrap();

    let mut before = workdeck();
    let before = before
        .env("XDG_CONFIG_HOME", config.path())
        .arg("--cwd")
        .arg(&nested)
        .args(["extension", "list", "--json"])
        .output()
        .unwrap();
    assert!(
        before.status.success(),
        "{}",
        String::from_utf8_lossy(&before.stderr)
    );
    let before: Value = serde_json::from_slice(&before.stdout).unwrap();
    let before_ids = before["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(before_ids, ["user-path", "global-path"]);

    let mut trust = workdeck();
    trust
        .env("XDG_CONFIG_HOME", config.path())
        .arg("--cwd")
        .arg(&nested)
        .args(["extension", "trust", "--allow", "--yes", "--repo"])
        .arg(&repo)
        .assert()
        .success();

    let mut after = workdeck();
    let after = after
        .env("XDG_CONFIG_HOME", config.path())
        .arg("--cwd")
        .arg(&nested)
        .args(["extension", "list", "--json"])
        .output()
        .unwrap();
    assert!(
        after.status.success(),
        "{}",
        String::from_utf8_lossy(&after.stderr)
    );
    let after: Value = serde_json::from_slice(&after.stdout).unwrap();
    let after_ids = after["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["id"].as_str())
        .collect::<Vec<_>>();
    assert_eq!(after_ids, ["user-path", "global-path", "repo-path"]);
}

#[test]
fn managed_extension_cli_installs_lists_updates_and_removes_native_repository() {
    let source_root = tempdir().unwrap();
    let source = source_root.path().join("managed-native");
    let config = tempdir().unwrap();
    fs::create_dir_all(source.join("bin")).unwrap();
    git(&source, &["init"]);
    git(&source, &["config", "user.email", "workdeck@example.test"]);
    git(&source, &["config", "user.name", "Workdeck Test"]);
    fs::write(
        source.join("workdeck-extension.toml"),
        "id = 'managed-native'\nname = 'Managed native'\nversion = '1.0.0'\napi_version = 1\nexecutable = 'bin/managed-native'\ncapabilities = []\n",
    )
    .unwrap();
    fs::write(source.join("bin/managed-native"), "fixture executable\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial"]);

    let mut install = workdeck();
    install
        .env("XDG_CONFIG_HOME", config.path())
        .args(["extension", "install"])
        .arg(&source)
        .args(["--yes", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\": \"extension_install\""))
        .stdout(predicate::str::contains("\"version\": \"1.0.0\""));

    let installed = config
        .path()
        .join("workdeck/extensions/installed/managed-native");
    assert!(installed.join("workdeck-extension.toml").is_file());
    let records: Value = serde_json::from_str(
        &fs::read_to_string(
            config
                .path()
                .join("workdeck/extensions/installed/records.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        records["installs"]["managed-native"]["cloneUrl"],
        source.to_string_lossy().as_ref()
    );

    let mut list = workdeck();
    list.env("XDG_CONFIG_HOME", config.path())
        .args(["extension", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"managed-native\""))
        .stdout(predicate::str::contains("\"managed\": true"));

    fs::write(
        source.join("workdeck-extension.toml"),
        "id = 'managed-native'\nname = 'Managed native'\nversion = '1.1.0'\napi_version = 1\nexecutable = 'bin/managed-native'\ncapabilities = []\n",
    )
    .unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "update"]);

    let mut update = workdeck();
    update
        .env("XDG_CONFIG_HOME", config.path())
        .args(["extension", "update", "managed-native", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changed\": true"))
        .stdout(predicate::str::contains("\"version\": \"1.1.0\""));

    let mut remove = workdeck();
    remove
        .env("XDG_CONFIG_HOME", config.path())
        .args(["extension", "remove", "managed-native", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\": \"extension_remove\""));
    assert!(!installed.exists());
}

#[test]
fn managed_extension_install_requires_explicit_consent_without_a_terminal() {
    let config = tempdir().unwrap();
    let mut install = workdeck();
    install
        .env("XDG_CONFIG_HOME", config.path())
        .args(["extension", "install", "acme/native-extension"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("re-run with --yes"));
    assert!(!config.path().join("workdeck/extensions/installed").exists());
}

#[test]
fn extension_help_exposes_complete_native_management_lifecycle() {
    workdeck()
        .args(["extension", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("update"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("trust"));
}

#[test]
fn pager_plain_text_fallback_is_headless_sanitized_and_read_only() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    let mut command = assert_cmd::Command::cargo_bin("workdeck").unwrap();
    command
        .env("HOME", "/nonexistent/workdeck-test-home")
        .arg("--cwd")
        .arg(dir.path())
        .arg("pager")
        .write_stdin("plain\x1b]52;c;SGVsbG8=\x07 output\x1b[2J")
        .assert()
        .success()
        .stdout("plain output");

    assert!(!dir.path().join(".agents/workdeck").exists());
}

#[test]
fn pager_redirected_diff_passthrough_preserves_pipeline_exit_and_read_only_behavior() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    let patch = "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1 +1 @@\n-old\n+new\n";
    let mut command = assert_cmd::Command::cargo_bin("workdeck").unwrap();
    command
        .env("HOME", "/nonexistent/workdeck-test-home")
        .arg("--cwd")
        .arg(dir.path())
        .arg("pager")
        .write_stdin(patch)
        .assert()
        .success()
        .stdout(patch)
        .stderr(predicate::str::is_empty());
    assert!(!dir.path().join(".agents/workdeck").exists());
}

#[test]
fn captured_pager_redirect_preserves_sgr_but_strips_terminal_commands() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    let patch =
        "\x1b[31mdiff --git a/a b/a\x1b[0m\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\x1b[2J\n";
    let mut command = assert_cmd::Command::cargo_bin("workdeck").unwrap();
    command
        .env("HOME", "/nonexistent/workdeck-test-home")
        .env("TERM", "dumb")
        .env("LV", "-c")
        .arg("--cwd")
        .arg(dir.path())
        .arg("pager")
        .write_stdin(patch)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\x1b[31mdiff --git a/a b/a\x1b[0m",
        ))
        .stdout(predicate::str::contains("\x1b[2J").not())
        .stderr(predicate::str::is_empty());
    assert!(!dir.path().join(".agents/workdeck").exists());
}

#[test]
fn markup_render_defaults_to_stdin_and_emits_hunks_exact_json_shape() {
    let mut command = assert_cmd::Command::cargo_bin("workdeck").unwrap();
    command
        .env("HOME", "/nonexistent/workdeck-test-home")
        .args(["markup", "render", "--width", "12", "--json"])
        .write_stdin("<box border>hi</box>")
        .assert()
        .success()
        .stdout(concat!(
            "{\n",
            "  \"width\": 12,\n",
            "  \"lines\": [\n",
            "    \"┌──────────┐\",\n",
            "    \"│hi        │\",\n",
            "    \"└──────────┘\"\n",
            "  ],\n",
            "  \"notes\": []\n",
            "}\n"
        ))
        .stderr(predicate::str::is_empty());
}

#[test]
fn hunk_stml_cli_oracle_runs_exactly_under_workdeck_naming() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../port/hunk/oracles/stml-cli.json")).unwrap();
    assert_eq!(
        fixture["baseline"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    for run in fixture["runs"].as_array().unwrap() {
        let input = &run["input"];
        let mut args = vec![
            "markup".to_owned(),
            "render".to_owned(),
            input["file"].as_str().unwrap().to_owned(),
            "--width".to_owned(),
            input["width"].as_u64().unwrap().to_string(),
            "--color".to_owned(),
            input["color"].as_str().unwrap().to_owned(),
        ];
        if let Some(theme) = input["theme"].as_str() {
            args.extend(["--theme".to_owned(), theme.to_owned()]);
        }
        if input["json"].as_bool().unwrap() {
            args.push("--json".into());
        }
        let output = assert_cmd::Command::cargo_bin("workdeck")
            .unwrap()
            .env("HOME", "/nonexistent/workdeck-test-home")
            .args(args)
            .write_stdin(run["markup"].as_str().unwrap())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            run["exit"].as_i64().map(|code| code as i32),
            "input: {input}"
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            run["stdout"].as_str().unwrap(),
            "input: {input}"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            run["stderr"].as_str().unwrap(),
            "input: {input}"
        );
    }
}

#[test]
fn markup_render_routes_degradation_notes_to_stderr_and_reads_relative_files() {
    let directory = tempdir().unwrap();
    fs::write(directory.path().join("note.stml"), "<wat>x</wat>").unwrap();
    let mut command = assert_cmd::Command::cargo_bin("workdeck").unwrap();
    command
        .env("HOME", "/nonexistent/workdeck-test-home")
        .current_dir(directory.path())
        .args(["markup", "render", "note.stml", "--color", "never"])
        .assert()
        .success()
        .stdout("x\n")
        .stderr("note: unknown tag <wat>\n");
}

#[test]
fn markup_render_always_color_uses_span_styles_even_in_json() {
    let markup = "<b><c fg=\"success\">ok</c></b>";
    let mut colored = assert_cmd::Command::cargo_bin("workdeck").unwrap();
    colored
        .env("HOME", "/nonexistent/workdeck-test-home")
        .args([
            "markup",
            "render",
            "-",
            "--color",
            "always",
            "--theme",
            "github-dark-default",
        ])
        .write_stdin(markup)
        .assert()
        .success()
        .stdout("\x1b[1;38;2;46;160;67mok\x1b[0m\n")
        .stderr(predicate::str::is_empty());

    let mut json = assert_cmd::Command::cargo_bin("workdeck").unwrap();
    let output = json
        .env("HOME", "/nonexistent/workdeck-test-home")
        .args(["markup", "render", "-", "--color", "always", "--json"])
        .write_stdin(markup)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.contains(&0x1b));
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        payload["lines"],
        serde_json::json!(["\u{1b}[1;38;2;46;160;67mok\u{1b}[0m"])
    );
}

#[test]
fn markup_guide_is_headless_and_contains_the_reference_width_contract() {
    workdeck()
        .args(["markup", "guide"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "# STML — terminal markup for Workdeck agent notes\n",
        ))
        .stdout(predicate::str::contains("Design for ~56 cols"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn outside_git_repo_prints_actionable_error() {
    let dir = tempdir().unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Workdeck must be run inside a Git repository",
        ))
        .stderr(predicate::str::contains("workdeck --cwd <repo-path>"))
        .stderr(predicate::str::contains("git init"));
}

#[test]
fn init_creates_agents_workdeck_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--init")
        .assert()
        .success()
        .stdout(predicate::str::contains(".agents/workdeck"));

    assert!(dir.path().join(".agents/workdeck/config.toml").exists());
    assert!(dir.path().join(".agents/workdeck/issues").is_dir());
    assert!(dir.path().join(".agents/workdeck/agents").is_dir());
    let config = fs::read_to_string(dir.path().join(".agents/workdeck/config.toml")).unwrap();
    assert!(config.contains("group_changes = \"g\""));
    assert!(config.contains("toggle_dirstat = \"w\""));
    assert!(config.contains("[git]"));
    assert!(config.contains("[refresh]"));
    assert!(config.contains("git = \"G\""));
    assert!(config.contains("recent_commits = 30"));
    assert!(config.contains("interval_ms = 1500"));
}

#[test]
fn status_json_reports_untracked_files_without_mutating_git() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::write(dir.path().join("new-file.txt"), "hello\n").unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--status-json")
        .assert()
        .success()
        .stdout(predicate::str::contains("new-file.txt"))
        .stdout(predicate::str::contains("untracked"))
        .stdout(predicate::str::contains("\"counts\""))
        .stdout(predicate::str::contains("\"groups\""));

    let status = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .arg("status")
        .arg("--short")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&status.stdout), "?? new-file.txt\n");
}

#[test]
fn status_json_reports_compact_stage_label() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    git(
        dir.path(),
        &["config", "user.email", "workdeck@example.test"],
    );
    git(dir.path(), &["config", "user.name", "Workdeck Test"]);
    fs::write(dir.path().join("file.txt"), "one\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    fs::write(dir.path().join("file.txt"), "one\ntwo\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    fs::write(dir.path().join("file.txt"), "one\ntwo\nthree\n").unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--status-json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"stage\": \"staged+unstaged\""))
        .stdout(predicate::str::contains("\"staged\": true"))
        .stdout(predicate::str::contains("\"unstaged\": true"));
}

#[test]
fn doctor_reports_valid_repo_without_creating_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"))
        .stdout(predicate::str::contains("\"name\": \"config\""));

    assert!(!dir.path().join(".agents/workdeck").exists());
}

#[test]
fn invalid_config_keybinding_fails_before_tui() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::create_dir_all(dir.path().join(".agents/workdeck")).unwrap();
    fs::write(
        dir.path().join(".agents/workdeck/config.toml"),
        r#"
        [keys]
        quit = "q"
        files = "q"
        "#,
    )
    .unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("fail  config"))
        .stdout(predicate::str::contains("duplicate key binding"))
        .stderr(predicate::str::contains("doctor found failed checks"));
}

#[test]
fn doctor_reports_corrupt_store_data_as_failed_checks() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::create_dir_all(dir.path().join(".agents/workdeck/issues")).unwrap();
    fs::write(
        dir.path().join(".agents/workdeck/issues/WD-1.toml"),
        "not valid toml =",
    )
    .unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor", "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"ok\": false"))
        .stdout(predicate::str::contains("\"code\": \"doctor_failed\""))
        .stdout(predicate::str::contains("\"name\": \"issues\""))
        .stdout(predicate::str::contains("failed to parse issue"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn issue_commands_manage_file_backed_issues() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "create",
            "Render changes",
            "--status",
            "in-progress",
            "--priority",
            "high",
            "--due-at",
            "2026-05-31",
            "--label",
            "git,mvp",
            "--commit",
            "abc123",
            "--file",
            "src/main.rs",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"WD-1\""))
        .stdout(predicate::str::contains("\"status\": \"in-progress\""))
        .stdout(predicate::str::contains("\"due_at\": \"2026-05-31\""))
        .stdout(predicate::str::contains("\"abc123\""))
        .stdout(predicate::str::contains("\"src/main.rs\""));

    assert!(
        dir.path()
            .join(".agents/workdeck/issues/WD-1.toml")
            .exists()
    );

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "update",
            "WD-1",
            "--title",
            "Render nested changes",
            "--priority",
            "urgent",
            "--commit",
            "def456,abc123",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Render nested changes"))
        .stdout(predicate::str::contains("\"priority\": \"urgent\""))
        .stdout(predicate::str::contains("\"def456\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "link", "WD-1", "src/lib.rs", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"src/lib.rs\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WD-1"))
        .stdout(predicate::str::contains("Render nested changes"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "show", "WD-1", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"WD-1\""))
        .stdout(predicate::str::contains("\"src/main.rs\""))
        .stdout(predicate::str::contains("\"src/lib.rs\""))
        .stdout(predicate::str::contains("\"abc123\""))
        .stdout(predicate::str::contains("\"def456\""));
}

#[test]
fn issue_command_rejects_invalid_status() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "create", "Bad status", "--status", "wat"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown status"));
}

#[test]
fn reference_commands_manage_projects_cycles_and_labels() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "project",
            "save",
            "Workdeck MVP",
            "--description",
            "Initial local release",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"workdeck-mvp\""))
        .stdout(predicate::str::contains("Initial local release"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "cycle",
            "save",
            "MVP",
            "--id",
            "mvp",
            "--starts-at",
            "2026-05-24",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"starts_at\": \"2026-05-24\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["label", "save", "Git", "--color", "green", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"git\""))
        .stdout(predicate::str::contains("\"color\": \"green\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("workdeck-mvp"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "1 project(s), 1 cycle(s), 1 label(s)",
        ));

    assert!(dir.path().join(".agents/workdeck/projects.toml").exists());
    assert!(dir.path().join(".agents/workdeck/cycles.toml").exists());
    assert!(dir.path().join(".agents/workdeck/labels.toml").exists());
}

#[test]
fn agent_commands_record_list_and_show_sessions() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "agent",
            "record",
            "Implement shell",
            "--id",
            "session-1",
            "--agent",
            "codex",
            "--status",
            "done",
            "--goal",
            "Build TUI shell",
            "--summary",
            "Implemented tabs",
            "--plan",
            "Inspect repo",
            "--plan",
            "Build shell",
            "--file",
            "src/main.rs",
            "--command",
            "cargo test",
            "--test",
            "cargo test",
            "--note",
            "Continue with previews",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"session-1\""))
        .stdout(predicate::str::contains("\"src/main.rs\""))
        .stdout(predicate::str::contains("Inspect repo"));

    assert!(
        dir.path()
            .join(".agents/workdeck/agents/session-1.toml")
            .exists()
    );

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session-1"))
        .stdout(predicate::str::contains("Implement shell"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "show", "session-1", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"agent\": \"codex\""))
        .stdout(predicate::str::contains("Continue with previews"));
}

#[test]
fn agent_import_reads_jsonl_sessions() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    let jsonl = dir.path().join("sessions.jsonl");
    fs::write(
        &jsonl,
        r#"{"session":{"id":"session-jsonl-1","title":"Imported JSONL","agent":"codex","cwd":"/tmp/workdeck","status":"done","started_at":"2026-05-24T12:00:00Z","goal":"Import logs","plan":["parse jsonl"],"touched_files":[{"path":"src/main.rs","change_type":"modified"}],"tests_run":["cargo test"],"handoff_notes":["review import"]}}
{"id":"session-jsonl-2","title":"Imported direct","agent":"codex","status":"active","started_at":"2026-05-24T13:00:00Z"}
"#,
    )
    .unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "import"])
        .arg(&jsonl)
        .arg("--json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"session-jsonl-1\""))
        .stdout(predicate::str::contains("\"id\": \"session-jsonl-2\""))
        .stdout(predicate::str::contains("parse jsonl"));

    assert!(
        dir.path()
            .join(".agents/workdeck/agents/session-jsonl-1.toml")
            .exists()
    );

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session-jsonl-1"))
        .stdout(predicate::str::contains("session-jsonl-2"));
}

#[test]
fn export_emits_json_and_jsonl_without_mutating_empty_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["export"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"issues\": []"))
        .stdout(predicate::str::contains("\"agent_sessions\": []"));

    assert!(!dir.path().join(".agents/workdeck").exists());

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "create",
            "Export local data",
            "--project",
            "workdeck",
            "--json",
        ])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "record", "Export run", "--id", "export-run"])
        .assert()
        .success();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["export", "--jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\":\"issue\""))
        .stdout(predicate::str::contains("\"kind\":\"agent_session\""))
        .stdout(predicate::str::contains("\"kind\":\"event\""));
}

#[test]
fn repo_read_only_commands_do_not_create_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "ignore me\n").unwrap();

    let output = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["files", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let files: Value = serde_json::from_slice(&output).unwrap();
    assert!(files.to_string().contains("README.md"));
    assert!(files.to_string().contains("src"));
    assert!(!files.to_string().contains("ignored.txt"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["files", "show", "README.md", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"content\": \"hello\\n\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changes\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["changes", "list", "--group", "status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"untracked\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["search", "main", "--target", "files", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"));

    assert!(!dir.path().join(".agents/workdeck").exists());
}

#[test]
fn json_commands_use_success_and_error_envelopes() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();

    let status = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let status_json: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_json["ok"], true);
    assert_eq!(status_json["kind"], "status");
    assert!(status_json["data"]["changes"].is_array());

    let created = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "create", "Envelope issue", "--json"])
        .output()
        .unwrap();
    assert!(created.status.success());
    let created_json: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(created_json["ok"], true);
    assert_eq!(created_json["kind"], "issue");
    assert_eq!(created_json["action"], "create");
    assert_eq!(created_json["data"]["key"], "WD-1");

    let missing = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "show", "WD-404", "--json"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(3));
    let missing_json: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(missing_json["ok"], false);
    assert_eq!(missing_json["error"]["code"], "not_found");
    assert!(
        missing_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("WD-404")
    );
    assert!(String::from_utf8_lossy(&missing.stderr).is_empty());
}

#[test]
fn issue_commands_cover_headless_lifecycle() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "create", "Ship CLI", "--file", "src/main.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WD-1"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "assign", "WD-1", "rutger"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "label", "add", "WD-1", "cli"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "link-commit", "WD-1", "abc123"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "close", "WD-1"])
        .assert()
        .success();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue", "list", "--status", "done", "--label", "cli", "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"WD-1\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "unlink-file", "WD-1", "src/main.rs"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "unlink-commit", "WD-1", "abc123"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "reopen", "WD-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WD-1"));

    let issue_json = dir.path().join("issue.json");
    fs::write(
        &issue_json,
        r#"{
          "title": "JSON issue",
          "description": "Created without shell quoting",
          "status": "todo",
          "labels": ["json"],
          "linked_files": ["src/json.rs"]
        }"#,
    )
    .unwrap();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "create",
            "--from-json",
            issue_json.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"JSON issue\""))
        .stdout(predicate::str::contains("src/json.rs"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "show", "WD-404"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("issue WD-404 does not exist"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "delete", "WD-1", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("deleted WD-1"));
}

#[test]
fn reference_agent_config_events_and_import_commands_are_headless() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["project", "save", "Workdeck", "--id", "workdeck"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["cycle", "save", "MVP", "--id", "mvp"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["label", "save", "CLI", "--id", "cli", "--color", "green"])
        .assert()
        .success();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["project", "show", "workdeck", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"workdeck\""));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["label", "list", "--color", "green", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"cli\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "record", "Run CLI", "--id", "run-cli"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "append-plan", "run-cli", "Add commands"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "add-file", "run-cli", "src/main.rs"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "finish", "run-cli", "--summary", "Done", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"done\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "set", "ui.preview", "false", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"set\": true"));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "get", "ui.preview", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"value\": false"));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "set", "keys.tasks", "I", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"keys.issues\""));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "get", "keys.issues", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"value\": \"I\""));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["events", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("agent_session_saved"));

    let export = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["export"])
        .output()
        .unwrap();
    assert!(export.status.success());
    let export_path = dir.path().join("export.json");
    fs::write(&export_path, export.stdout).unwrap();

    let import_dir = tempdir().unwrap();
    git(import_dir.path(), &["init"]);
    workdeck()
        .arg("--cwd")
        .arg(import_dir.path())
        .args([
            "import",
            export_path.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"projects\": 1"));
    workdeck()
        .arg("--cwd")
        .arg(import_dir.path())
        .args(["import", export_path.to_str().unwrap(), "--replace"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(import_dir.path())
        .args(["agent", "show", "run-cli"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Run CLI"));
}

fn git(cwd: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}
