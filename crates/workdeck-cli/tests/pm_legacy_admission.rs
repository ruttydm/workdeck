#[path = "support/legacy.rs"]
mod legacy_fixture;
use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use workdeck_cli::store::{AgentSession, WorkdeckStore};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .env("EDITOR", "must-never-start-a-legacy-editor")
        .args(args)
        .output()
        .unwrap()
}
fn files(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, current: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == ".git" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                out.insert(path.strip_prefix(root).unwrap().to_owned(), None);
                visit(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    Some(fs::read(path).unwrap()),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
fn fixture(custom: bool) -> (tempfile::TempDir, WorkdeckStore) {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let store = WorkdeckStore::new(root.path().join(if custom {
        "planning-data"
    } else {
        ".agents/workdeck"
    }));
    legacy_fixture::init(store.root());
    legacy_fixture::issue(store.root(), "WD-1", "Legacy issue");
    legacy_fixture::write(
        store.root(),
        "projects.toml",
        &json!({"projects":[{"id":"project","name":"Project","created_at":"2026-09-01T00:00:00Z","updated_at":"2026-09-01T00:00:00Z"}]}),
    );
    legacy_fixture::write(
        store.root(),
        "cycles.toml",
        &json!({"cycles":[{"id":"cycle","name":"Cycle"}]}),
    );
    legacy_fixture::write(
        store.root(),
        "labels.toml",
        &json!({"labels":[{"id":"label","name":"Label"}]}),
    );
    let mut session = AgentSession::new("Legacy session".into());
    session.id = "session".into();
    legacy_fixture::write(store.root(), "agents/session.toml", &session);
    if custom {
        fs::create_dir(root.path().join(".workdeck")).unwrap();
        fs::write(
            root.path().join(".workdeck/config.toml"),
            "[paths]\ndata_dir='planning-data'\n",
        )
        .unwrap();
    }
    fs::write(root.path().join("input.json"), r#"{"issues":[]}"#).unwrap();
    (root, store)
}
fn rejected(root: &Path, args: &[&str]) {
    let before = files(root);
    let output = run(root, args);
    assert_eq!(
        files(root),
        before,
        "command changed legacy source: {args:?}"
    );
    assert_eq!(
        output.status.code(),
        Some(6),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["api_version"], 1);
    assert_eq!(
        result["error"]["code"], "legacy_store",
        "{args:?}: {result}"
    );
    let hint = result["error"]["hint"].as_str().unwrap();
    assert!(
        hint.contains("migrate legacy") && hint.contains("--plan-out") && hint.contains("--apply"),
        "{hint}"
    );
    assert!(!root.join(".workdeck/config.yml").exists());
}
#[test]
fn invalid_create_is_rejected_before_creating_a_partial_issue() {
    for custom in [false, true] {
        let (root, _) = fixture(custom);
        rejected(
            root.path(),
            &[
                "issue",
                "create",
                "Bad status",
                "--status",
                "not-a-status",
                "--json",
            ],
        );
    }
}
#[test]
fn every_legacy_mutation_family_is_rejected_even_with_native_options_or_replace() {
    let cases: &[&[&str]] = &[
        &["context", "--issue", "WD-1"],
        &["next", "--issue", "WD-1"],
        &["issue", "next"],
        &["question", "list"],
        &["question", "create", "input.json"],
        &["handoff", "list", "--issue", "WD-1"],
        &["handoff", "create", "input.json"],
        &["issue", "parent", "WD-1", "WD-2"],
        &["issue", "prerequisite", "WD-1", "add", "WD-2"],
        &[
            "issue",
            "prerequisite",
            "WD-1",
            "remove",
            "WD-2",
            "--reason",
            "Resolved",
        ],
        &["issue", "relate", "WD-1", "WD-2"],
        &["issue", "unrelate", "WD-1", "WD-2"],
        &[
            "issue",
            "update",
            "WD-1",
            "--feature",
            "FEAT-01ARZ3NDEKTSV4RRFFQ69G5FAV",
        ],
        &[
            "issue",
            "update",
            "WD-1",
            "--gate",
            "GATE-01ARZ3NDEKTSV4RRFFQ69G5FAV",
        ],
        &["feature", "create", "No write"],
        &[
            "feature",
            "update",
            "FEAT-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "--name",
            "No write",
        ],
        &[
            "feature",
            "move",
            "FEAT-01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "group",
        ],
        &["gate", "create", "input.json"],
        &["evidence", "declare", "input.json"],
        &["issue", "create", "No write"],
        &["issue", "create", "No write", "--request-id", "stable"],
        &["issue", "update", "WD-1", "--title", "No write"],
        &["issue", "create", "No write", "--milestone", "milestone"],
        &["issue", "create", "No write", "--target", "target"],
        &["issue", "update", "WD-1", "--milestone", "milestone"],
        &["issue", "update", "WD-1", "--target", "target"],
        &["issue", "update", "WD-1", "--clear", "project"],
        &["issue", "update", "WD-1", "--unset", "targets"],
        &["issue", "custom", "WD-1", "--set", "count=1"],
        &["issue", "estimate", "WD-1", "2", "--unit", "hours"],
        &["project", "custom", "project", "--set", "count=1"],
        &["cycle", "custom", "cycle", "--set", "count=1"],
        &["label", "custom", "label", "--set", "count=1"],
        &["initiative", "custom", "initiative", "--set", "count=1"],
        &["milestone", "custom", "milestone", "--set", "count=1"],
        &["target", "custom", "target", "--set", "count=1"],
        &["issue", "edit", "WD-1"],
        &["issue", "link", "WD-1", "input.json"],
        &["issue", "link-file", "WD-1", "input.json"],
        &["issue", "unlink-file", "WD-1", "input.json"],
        &["issue", "link-commit", "WD-1", "abc123"],
        &["issue", "unlink-commit", "WD-1", "abc123"],
        &["issue", "link-document", "WD-1", "input.json"],
        &["issue", "unlink-document", "WD-1", "input.json"],
        &["issue", "close", "WD-1"],
        &["issue", "done", "WD-1"],
        &["issue", "reopen", "WD-1"],
        &["issue", "cancel", "WD-1"],
        &["issue", "move", "WD-1", "--status", "todo"],
        &["issue", "assign", "WD-1", "actor"],
        &["issue", "unassign", "WD-1"],
        &["issue", "label", "add", "WD-1", "label"],
        &["issue", "label", "remove", "WD-1", "label"],
        &["issue", "comment", "WD-1", "comment", "--author", "actor"],
        &["issue", "attach", "WD-1", "input.json", "--author", "actor"],
        &["issue", "archive", "WD-1"],
        &["issue", "delete", "WD-1", "--yes"],
        &["project", "save", "Project"],
        &["project", "create", "Project"],
        &["project", "update", "project", "--name", "New"],
        &["project", "archive", "project"],
        &["project", "delete", "project", "--yes", "--force"],
        &["cycle", "save", "Cycle"],
        &["cycle", "create", "Cycle"],
        &["cycle", "update", "cycle", "--name", "New"],
        &["cycle", "archive", "cycle"],
        &["cycle", "delete", "cycle", "--yes", "--force"],
        &["label", "save", "Label"],
        &["label", "create", "Label"],
        &["label", "update", "label", "--name", "New"],
        &["label", "archive", "label"],
        &["label", "delete", "label", "--yes", "--force"],
        &["initiative", "create", "Initiative"],
        &["initiative", "update", "initiative", "--name", "New"],
        &["initiative", "archive", "initiative"],
        &["milestone", "create", "Milestone", "--project", "project"],
        &["milestone", "update", "milestone", "--name", "New"],
        &["milestone", "archive", "milestone"],
        &["target", "create", "Target"],
        &["target", "update", "target", "--name", "New"],
        &["target", "archive", "target"],
        &["agent", "record", "Session"],
        &["agent", "update", "session", "--title", "New"],
        &["agent", "finish", "session"],
        &["agent", "append-plan", "session", "Plan"],
        &["agent", "add-file", "session", "input.json"],
        &["agent", "add-command", "session", "command"],
        &["agent", "add-test", "session", "test"],
        &["agent", "add-note", "session", "note"],
        &["agent", "delete", "session", "--yes"],
        &["agent", "import", "input.json"],
        &["import", "input.json"],
        &["import", "input.json", "--replace"],
        &["import", "input.json", "--merge"],
    ];
    for custom in [false, true] {
        let (root, _) = fixture(custom);
        for args in cases {
            let mut args = args.to_vec();
            args.push("--json");
            rejected(root.path(), &args);
        }
    }
}
#[test]
fn legacy_readers_and_import_preview_stay_available_without_changing_source() {
    for custom in [false, true] {
        let (root, _) = fixture(custom);
        let before = files(root.path());
        for args in [
            vec!["issue", "list"],
            vec!["issue", "show", "WD-1"],
            vec!["project", "list"],
            vec!["project", "show", "project"],
            vec!["cycle", "list"],
            vec!["cycle", "show", "cycle"],
            vec!["label", "list"],
            vec!["label", "show", "label"],
            vec!["agent", "list"],
            vec!["agent", "show", "session"],
            vec!["events", "list"],
            vec!["export"],
            vec!["search", "Legacy", "--target", "issues"],
            vec!["import", "input.json", "--replace", "--dry-run"],
        ] {
            let mut args = args;
            args.push("--json");
            let output = run(root.path(), &args);
            assert!(
                output.status.success(),
                "{args:?}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(files(root.path()), before, "read changed source: {args:?}");
        }
    }
}

#[test]
fn native_membership_options_cannot_silently_return_legacy_record_only_views() {
    for custom in [false, true] {
        let (root, _) = fixture(custom);
        for kind in [
            "initiative",
            "project",
            "milestone",
            "cycle",
            "target",
            "label",
        ] {
            rejected(root.path(), &[kind, "show", kind, "--members", "--json"]);
            rejected(
                root.path(),
                &[
                    kind,
                    "show",
                    kind,
                    "--members",
                    "--archive",
                    "all",
                    "--json",
                ],
            );
        }
    }
}
#[test]
fn app_preferences_are_independent_of_read_only_legacy_planning() {
    let (root, store) = fixture(false);
    let issue = fs::read(store.root().join("issues/WD-1.toml")).unwrap();
    let output = run(
        root.path(),
        &["config", "set", "ui.preview", "false", "--json"],
    );
    assert!(output.status.success());
    assert_eq!(
        fs::read(store.root().join("issues/WD-1.toml")).unwrap(),
        issue
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"]["set"],
        json!(true)
    );
}

#[test]
fn native_session_import_keeps_legacy_wrappers_and_nested_metadata() {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    assert!(run(root.path(), &["init", "--json"]).status.success());
    let top = json!({"id":"wrapped-top","title":"Top","producer":{"id":7},"touched_files":[{"path":"src/a.rs","change_type":"modified","extra":{"lines":3}}]});
    let nested = json!({"id":"wrapped-payload","title":"Payload","producer":{"id":8}});
    let direct = json!({"id":"direct-extra","title":"Direct","session":{"unknown":"kept"}});
    let text = [
        json!({"session":top}),
        json!({"payload":{"session":nested}}),
        direct.clone(),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n");
    fs::write(root.path().join("sessions.jsonl"), text).unwrap();
    let output = run(
        root.path(),
        &["agent", "import", "sessions.jsonl", "--json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    for original in [top, nested, direct] {
        let shown = run(
            root.path(),
            &["agent", "show", original["id"].as_str().unwrap(), "--json"],
        );
        assert!(shown.status.success());
        let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
        for (key, value) in original.as_object().unwrap() {
            assert_eq!(&shown["result"]["session"][key], value);
        }
    }
    fs::write(root.path().join("ambiguous.json"), r#"{"session":{"id":"one","title":"One"},"payload":{"session":{"id":"two","title":"Two"}}}"#).unwrap();
    let before = files(root.path());
    let output = run(
        root.path(),
        &["agent", "import", "ambiguous.json", "--json"],
    );
    assert!(!output.status.success());
    assert_eq!(files(root.path()), before);
}

#[test]
fn legacy_read_configuration_errors_keep_the_existing_exit_and_envelope() {
    let (root, _) = fixture(false);
    fs::create_dir(root.path().join(".workdeck")).unwrap();
    fs::write(root.path().join(".workdeck/config.toml"), "[").unwrap();
    let before = files(root.path());
    for args in [
        vec!["issue", "show", "WD-1", "--json"],
        vec!["issue", "list", "--json"],
        vec!["project", "list", "--json"],
    ] {
        let output = run(root.path(), &args);
        assert_eq!(
            output.status.code(),
            Some(5),
            "legacy config exit changed: {args:?}"
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], "config_or_store_error");
        assert!(value.get("api_version").is_none());
    }
    let output = run(root.path(), &["next", "--issue", "WD-1", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["error"]["code"], "invalid_schema");
    assert_eq!(files(root.path()), before);
}
