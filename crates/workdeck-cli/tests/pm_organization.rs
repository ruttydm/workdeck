use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}
fn success(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    value
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    root
}
fn files(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn walk(root: &Path, current: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                out.insert(relative, None);
                walk(root, &path, out);
            } else {
                out.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
fn schema(root: &Path, input: &Value, request: &str) -> Value {
    fs::write(
        root.join("schema-change.json"),
        serde_json::to_vec(input).unwrap(),
    )
    .unwrap();
    let plan = success(
        root,
        &[
            "organization",
            "schema",
            "preview",
            "schema-change.json",
            "--json",
        ],
    );
    success(
        root,
        &[
            "organization",
            "schema",
            "apply",
            "schema-change.json",
            "--expected-preview",
            plan["result"]["fingerprint"].as_str().unwrap(),
            "--request-id",
            request,
            "--json",
        ],
    )
}

#[test]
fn user_commands_preserve_unknown_metadata_and_return_aggregate_source_tokens() {
    let root = fixture();
    let root = root.path();
    let empty = success(root, &["user", "list", "--json"]);
    assert!(empty["result"]["source"].is_null());
    assert!(!root.join(".workdeck/users.yml").exists());
    fs::write(root.join("user.json"),r#"{"name":"Agent One","kind":"agent","custom":{"team":"api","unknown":{"flag":true}},"x-badge":"blue"}"#).unwrap();
    let args = [
        "user",
        "create",
        "agent",
        "--from-json",
        "user.json",
        "--request-id",
        "user-once",
        "--json",
    ];
    let created = success(root, &args);
    let revision = created["result"]["source"]["revision"].to_string();
    let content = created["result"]["source"]["content"].as_str().unwrap();
    success(
        root,
        &[
            "user",
            "update",
            "agent",
            "--name",
            "Renamed",
            "--set",
            r#"team="platform""#,
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
    );
    let shown = success(root, &["user", "show", "agent", "--json"]);
    assert_eq!(shown["result"]["name"], "Renamed");
    assert_eq!(shown["result"]["kind"], "agent");
    assert_eq!(shown["result"]["custom"]["team"], "platform");
    assert_eq!(shown["result"]["custom"]["unknown"]["flag"], true);
    assert_eq!(shown["result"]["x-badge"], "blue");
    assert_eq!(success(root, &args)["result"], created["result"]);
    let before = files(&root.join(".workdeck"));
    let stale = run(
        root,
        &[
            "user",
            "archive",
            "agent",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
    );
    assert_eq!(stale.status.code(), Some(4));
    assert_eq!(files(&root.join(".workdeck")), before);
    success(root, &["user", "archive", "agent", "--json"]);
    assert_eq!(
        success(root, &["user", "show", "agent", "--json"])["result"]["archived"],
        true
    );
    success(root, &["user", "archive", "agent", "--restore", "--json"]);
    success(root, &["user", "mode", "registered", "--json"]);
    assert_eq!(
        success(root, &["user", "list", "--json"])["result"]["registry"]["mode"],
        "registered"
    );
    let before = files(&root.join(".workdeck"));
    assert!(
        !run(
            root,
            &[
                "issue",
                "create",
                "Unknown identity",
                "--assignee",
                "missing",
                "--json"
            ]
        )
        .status
        .success()
    );
    assert_eq!(files(&root.join(".workdeck")), before);
    let assigned = success(
        root,
        &[
            "issue",
            "create",
            "Registered identity",
            "--assignee",
            "agent",
            "--json",
        ],
    );
    assert_eq!(assigned["result"]["metadata"]["assignee"], "agent");
}

#[test]
fn schema_preview_custom_patches_and_compliance_share_repository_policy() {
    let root = fixture();
    let root = root.path();
    let issue = success(root, &["issue", "create", "Existing", "--json"]);
    let id = issue["result"]["metadata"]["id"].as_str().unwrap();
    let change = json!({"fields":{"risk":{"type":"enum","scopes":["issue","project"],"required":true,"options":["low","high"]}}});
    fs::write(
        root.join("schema-change.json"),
        serde_json::to_vec(&change).unwrap(),
    )
    .unwrap();
    let before = files(&root.join(".workdeck"));
    let blocked = success(
        root,
        &[
            "organization",
            "schema",
            "preview",
            "schema-change.json",
            "--json",
        ],
    );
    assert_eq!(blocked["result"]["allowed"], false);
    assert_eq!(files(&root.join(".workdeck")), before);
    let rejected = run(
        root,
        &[
            "organization",
            "schema",
            "apply",
            "schema-change.json",
            "--expected-preview",
            blocked["result"]["fingerprint"].as_str().unwrap(),
            "--json",
        ],
    );
    assert!(!rejected.status.success());
    assert_eq!(files(&root.join(".workdeck")), before);
    success(
        root,
        &[
            "issue",
            "custom",
            id,
            "--set",
            r#"risk="low""#,
            "--set",
            r#"opaque={"keep":true}"#,
            "--json",
        ],
    );
    let applied = schema(root, &change, "activate-risk");
    assert!(applied["receipt"]["operation_id"].is_string());
    let compliant = success(root, &["organization", "compliance", "--check", "--json"]);
    assert_eq!(compliant["result"]["compliant"], true);
    let before = files(&root.join(".workdeck"));
    for args in [
        vec!["issue", "custom", id, "--unset", "risk", "--json"],
        vec!["issue", "custom", id, "--set", "risk=42", "--json"],
        vec![
            "issue",
            "custom",
            id,
            "--set",
            r#"risk="high""#,
            "--unset",
            "risk",
            "--json",
        ],
        vec!["issue", "custom", id, "--set", "opaque={broken", "--json"],
    ] {
        assert!(!run(root, &args).status.success());
        assert_eq!(files(&root.join(".workdeck")), before);
    }
    success(
        root,
        &["issue", "custom", id, "--set", r#"risk="high""#, "--json"],
    );
    assert_eq!(
        success(root, &["issue", "show", id, "--json"])["result"]["metadata"]["custom"]["opaque"]["keep"],
        true
    );
    // Required project fields apply to the same semantic planning writer.
    let repository = workdeck_pm::Repository::discover(root).unwrap();
    success(
        root,
        &[
            "project",
            "create",
            "Project",
            "--id",
            "project",
            "--custom",
            r#"{"risk":"low","keep":1}"#,
            "--json",
        ],
    );
    success(
        root,
        &[
            "project",
            "custom",
            "project",
            "--set",
            r#"risk="high""#,
            "--json",
        ],
    );
    assert_eq!(
        success(root, &["project", "show", "project", "--json"])["result"]["metadata"]["custom"]["keep"],
        1
    );
    // Raw external edits remain inspectable and are reported, never auto-repaired.
    let path = repository
        .root()
        .join(issue["result"]["path"].as_str().unwrap());
    let text = fs::read_to_string(&path).unwrap();
    let document = workdeck_pm::documents::MarkdownDocument::parse(&path, &text).unwrap();
    let mut metadata: Value = document.deserialize().unwrap();
    assert_eq!(metadata["custom"]["risk"], "high");
    metadata["custom"]["risk"] = json!("invalid");
    fs::write(
        &path,
        format!("---\n{}\n---\n{}", metadata, document.body()),
    )
    .unwrap();
    let report = success(root, &["organization", "compliance", "--json"]);
    assert_eq!(report["result"]["compliant"], false);
    let check = run(root, &["organization", "compliance", "--check", "--json"]);
    assert_eq!(check.status.code(), Some(5));
    let value: Value = serde_json::from_slice(&check.stdout).unwrap();
    assert!(
        !value["error"]["details"]["violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn exact_estimates_report_per_unit_filter_archive_and_clear_without_losing_custom() {
    let root = fixture();
    let root = root.path();
    schema(
        root,
        &json!({"units":{"hours":{"name":"Hours"},"points":{"name":"Points"}},"unit_mode":"registered"}),
        "units",
    );
    let mut ids = Vec::new();
    for (value, unit) in [("0.1", "hours"), ("0.2", "hours"), ("3", "points")] {
        let issue = success(root, &["issue", "create", "Estimated", "--json"]);
        let id = issue["result"]["metadata"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        success(
            root,
            &["issue", "estimate", &id, value, "--unit", unit, "--json"],
        );
        ids.push(id);
    }
    success(
        root,
        &[
            "issue",
            "custom",
            &ids[0],
            "--set",
            r#"opaque={"keep":true}"#,
            "--json",
        ],
    );
    let report = success(root, &["organization", "estimate-report", "--json"]);
    assert_eq!(
        report["result"]["by_unit"],
        json!({"hours":"0.3","points":"3"})
    );
    success(root, &["issue", "archive", &ids[2], "--json"]);
    let active = success(
        root,
        &[
            "organization",
            "estimate-report",
            "--archive",
            "active",
            "--json",
        ],
    );
    assert_eq!(active["result"]["by_unit"], json!({"hours":"0.3"}));
    let before = files(&root.join(".workdeck"));
    for args in [
        vec![
            "issue", "estimate", &ids[0], "NaN", "--unit", "hours", "--json",
        ],
        vec![
            "issue", "estimate", &ids[0], "2", "--unit", "missing", "--json",
        ],
        vec![
            "issue",
            "estimate",
            &ids[0],
            "0.1234567",
            "--unit",
            "hours",
            "--json",
        ],
    ] {
        assert!(!run(root, &args).status.success());
        assert_eq!(files(&root.join(".workdeck")), before);
    }
    success(root, &["issue", "estimate", &ids[0], "--clear", "--json"]);
    assert!(
        success(root, &["issue", "show", &ids[0], "--json"])["result"]["metadata"]
            .get("estimate")
            .is_none()
    );
    assert_eq!(
        success(root, &["issue", "show", &ids[0], "--json"])["result"]["metadata"]["custom"]["opaque"]
            ["keep"],
        true
    );
    assert!(success(root, &["schema", "issue", "--json"])["result"].is_object());
}

#[test]
fn custom_patch_commands_preserve_other_keys_for_every_planning_kind() {
    let root = fixture();
    let root = root.path();
    success(
        root,
        &["project", "create", "Parent", "--id", "parent", "--json"],
    );
    for kind in [
        "initiative",
        "project",
        "milestone",
        "cycle",
        "target",
        "label",
    ] {
        let mut create = vec![
            kind,
            "create",
            kind,
            "--id",
            kind,
            "--custom",
            r#"{"keep":{"nested":true},"change":"before"}"#,
            "--json",
        ];
        if kind == "milestone" {
            create.extend(["--project", "parent"]);
        }
        success(root, &create);
        let args = [
            kind,
            "custom",
            kind,
            "--set",
            r#"change="after""#,
            "--request-id",
            kind,
            "--json",
        ];
        let patched = success(root, &args);
        assert_eq!(
            patched["result"]["metadata"]["custom"]["keep"]["nested"],
            true
        );
        assert_eq!(patched["result"]["metadata"]["custom"]["change"], "after");
        success(root, &[kind, "custom", kind, "--unset", "change", "--json"]);
        assert_eq!(success(root, &args)["result"], patched["result"]);
        let shown = success(root, &[kind, "show", kind, "--json"]);
        assert!(
            shown["result"]["metadata"]["custom"]
                .get("change")
                .is_none()
        );
        assert_eq!(
            shown["result"]["metadata"]["custom"]["keep"]["nested"],
            true
        );
    }
}

#[test]
fn schema_application_rejects_stale_review_and_replays_before_current_policy_reads() {
    let root = fixture();
    let root = root.path();
    let issue = success(root, &["issue", "create", "Planning", "--json"]);
    let id = issue["result"]["metadata"]["id"].as_str().unwrap();
    fs::write(
        root.join("change.json"),
        r#"{"fields":{"risk":{"type":"text","scopes":["issue"]}}}"#,
    )
    .unwrap();
    let plan = success(
        root,
        &["organization", "schema", "preview", "change.json", "--json"],
    );
    success(
        root,
        &[
            "issue",
            "update",
            id,
            "--title",
            "Changed after review",
            "--json",
        ],
    );
    let before = files(&root.join(".workdeck"));
    let stale = run(
        root,
        &[
            "organization",
            "schema",
            "apply",
            "change.json",
            "--expected-preview",
            plan["result"]["fingerprint"].as_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(stale.status.code(), Some(4));
    assert_eq!(files(&root.join(".workdeck")), before);
    let plan = success(
        root,
        &["organization", "schema", "preview", "change.json", "--json"],
    );
    let args = [
        "organization",
        "schema",
        "apply",
        "change.json",
        "--expected-preview",
        plan["result"]["fingerprint"].as_str().unwrap(),
        "--request-id",
        "schema-once",
        "--json",
    ];
    let applied = success(root, &args);
    success(
        root,
        &[
            "issue",
            "update",
            id,
            "--title",
            "Changed after apply",
            "--json",
        ],
    );
    assert_eq!(success(root, &args)["result"], applied["result"]);
    fs::write(
        root.join("change.json"),
        r#"{"fields":{},"unknown":"cannot disappear"}"#,
    )
    .unwrap();
    let before = files(&root.join(".workdeck"));
    assert!(!run(root, &args).status.success());
    assert_eq!(files(&root.join(".workdeck")), before);
}

#[test]
fn organization_commands_require_native_authority_and_do_not_initialize_or_write_legacy() {
    for source in ["fresh", "legacy", "custom"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        if source != "fresh" {
            let legacy = root.join(if source == "custom" {
                "planning-data"
            } else {
                ".agents/workdeck"
            });
            fs::create_dir_all(legacy.join("issues")).unwrap();
            if source == "custom" {
                fs::create_dir(root.join(".workdeck")).unwrap();
                fs::write(
                    root.join(".workdeck/config.toml"),
                    "[paths]\ndata_dir='planning-data'\n",
                )
                .unwrap();
            }
        }
        let before = files(root);
        for args in [
            vec!["user", "list", "--json"],
            vec!["user", "show", "person", "--json"],
            vec!["user", "create", "person", "Person", "--json"],
            vec!["user", "update", "person", "--name", "Renamed", "--json"],
            vec!["user", "archive", "person", "--json"],
            vec!["user", "mode", "registered", "--json"],
            vec!["organization", "schema", "show", "--json"],
            vec![
                "organization",
                "schema",
                "preview",
                "missing.json",
                "--json",
            ],
            vec![
                "organization",
                "schema",
                "apply",
                "missing.json",
                "--expected-preview",
                "missing",
                "--json",
            ],
            vec!["organization", "compliance", "--json"],
            vec!["organization", "estimate-report", "--json"],
        ] {
            let output = run(root, &args);
            assert!(!output.status.success(), "{args:?}");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                value["error"]["code"],
                if source == "fresh" {
                    "not_initialized"
                } else {
                    "legacy_store"
                },
                "{args:?}: {value}"
            );
            assert_eq!(files(root), before, "{args:?}");
        }
    }
}

#[test]
fn organization_catalog_advertises_real_commands_and_keeps_schema_introspection() {
    let temp = fixture();
    let root = temp.path();
    let before = files(&root.join(".workdeck"));
    let capabilities = success(root, &["capabilities", "--json"]);
    for feature in [
        "native_users",
        "organization_policy",
        "custom_patches",
        "unit_aware_estimates",
    ] {
        assert_eq!(
            capabilities["result"]["features"][feature], true,
            "{feature}"
        );
    }
    let commands = capabilities["result"]["commands"].as_array().unwrap();
    for (path, flag) in [
        ("user create", "from-json"),
        ("user update", "set"),
        ("organization schema apply", "expected-preview"),
        ("organization compliance", "check"),
        ("organization estimate-report", "archive"),
        ("issue estimate", "unit"),
        ("issue custom", "unset"),
        ("project custom", "set"),
    ] {
        let command = commands
            .iter()
            .find(|command| command["path"] == path)
            .unwrap();
        assert_eq!(command["native_planning"]["implemented"], true, "{path}");
        assert!(
            command["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| arg["long"] == flag),
            "{path}: {flag}"
        );
    }
    for name in [
        "users-registry",
        "organization-schema",
        "custom-patch",
        "estimate",
    ] {
        let schema = success(root, &["schema", name, "--json"]);
        assert!(schema["result"].is_object(), "{name}");
    }
    assert_eq!(files(&root.join(".workdeck")), before);
}

#[cfg(unix)]
#[test]
fn organization_input_files_reject_fifo_and_symlinks_without_blocking_or_writing() {
    use std::{os::unix::fs::symlink, time::Duration};
    let temp = fixture();
    let root = temp.path();
    fs::write(root.join("outside.json"), r#"{"name":"External"}"#).unwrap();
    symlink(root.join("outside.json"), root.join("link.json")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(root.join("input.json"))
            .status()
            .unwrap()
            .success()
    );
    let before = files(&root.join(".workdeck"));
    for path in ["input.json", "link.json"] {
        let output = assert_cmd::Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root)
            .env("XDG_CONFIG_HOME", root.join("isolated-config"))
            .timeout(Duration::from_secs(5))
            .args(["user", "create", "person", "--from-json", path, "--json"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{path}: {error}; status={:?}; stderr={}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert_eq!(value["error"]["code"], "unsafe_path");
        assert_eq!(files(&root.join(".workdeck")), before);
    }
}
