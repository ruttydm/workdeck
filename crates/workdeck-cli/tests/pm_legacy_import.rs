#[path = "support/legacy.rs"]
mod legacy_fixture;
// Legacy writes are closed by PM03. These tests author historical raw fixtures,
// qualify export and immutable preview, and reject actual CLI mutations. Native
// conversion/application remains covered by pm_legacy_conversion.rs.
use assert_cmd::prelude::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use workdeck_cli::store::WorkdeckStore;

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}

fn compatibility_preview(
    store: &WorkdeckStore,
    input: &[u8],
    replace: bool,
) -> anyhow::Result<workdeck_cli::store::LegacyImportSummary> {
    let workdeck_pm::ImportSource::Legacy(export) = workdeck_pm::decode_transfer(input)? else {
        anyhow::bail!("native input is not a legacy transfer");
    };
    store.preview_legacy_import(&export.canonical_document(), replace)
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

#[test]
fn legacy_replace_rejects_unknown_and_native_formats_before_touching_the_backlog() {
    let source = tempfile::tempdir().unwrap();
    let repository = workdeck_pm::Repository::init(source.path(), "WD").unwrap();
    let native = serde_json::to_value(repository.export_snapshot().unwrap()).unwrap();
    for value in [
        json!({}),
        json!({"unknown": []}),
        native.clone(),
        json!({"ok":true,"kind":"export","result":native}),
    ] {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        let store = WorkdeckStore::new(temp.path().join(".agents/workdeck"));
        legacy_fixture::init(store.root());
        legacy_fixture::issue(store.root(), "WD-1", "Keep original backlog");
        let before = files(store.root());
        fs::write(
            temp.path().join("input.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let result = run(
            temp.path(),
            &["import", "input.json", "--replace", "--json"],
        );
        assert!(
            !result.status.success(),
            "accepted unrecognized input: {}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert_eq!(files(store.root()), before);
        assert!(!temp.path().join(".workdeck/config.yml").exists());
    }
}

#[test]
fn legacy_export_envelopes_and_jsonl_preview_their_actual_records() {
    let source = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(source.path())
            .status()
            .unwrap()
            .success()
    );
    let source_store = WorkdeckStore::new(source.path().join(".agents/workdeck"));
    legacy_fixture::init(source_store.root());
    let issue = legacy_fixture::issue(source_store.root(), "WD-1", "Transfer legacy record");
    for format in ["--json", "--jsonl"] {
        let exported = run(source.path(), &["export", format]);
        assert!(exported.status.success());
        let target = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(target.path())
                .status()
                .unwrap()
                .success()
        );
        let store = WorkdeckStore::new(target.path().join(".agents/workdeck"));
        legacy_fixture::init(store.root());
        fs::write(target.path().join("input.data"), exported.stdout).unwrap();
        let before = files(store.root());
        let bytes = fs::read(target.path().join("input.data")).unwrap();
        let preview = compatibility_preview(&store, &bytes, false).unwrap();
        assert_eq!(preview.issues, 1);
        assert_eq!(files(store.root()), before);
        let workdeck_pm::ImportSource::Legacy(decoded) =
            workdeck_pm::decode_transfer(&bytes).unwrap()
        else {
            panic!("expected legacy export");
        };
        assert_eq!(decoded.canonical_document()["issues"][0]["key"], issue.key);
        assert_eq!(
            decoded.canonical_document()["issues"][0]["title"],
            issue.title
        );
    }
}

fn target() -> (tempfile::TempDir, WorkdeckStore) {
    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let store = WorkdeckStore::new(root.path().join(".agents/workdeck"));
    legacy_fixture::init(store.root());
    legacy_fixture::issue(store.root(), "WD-1", "Existing backlog");
    (root, store)
}

#[test]
fn every_invalid_record_and_serialization_failure_precedes_replacement() {
    let issue = serde_json::to_value(workdeck_cli::store::Issue::new(
        "WD-9".into(),
        "Imported".into(),
    ))
    .unwrap();
    let project = json!({"id":"project","name":"Project","created_at":"2001-01-01T00:00:00Z","updated_at":"2001-01-02T00:00:00Z"});
    let mut invalid_issue = issue.clone();
    invalid_issue["key"] = json!("../escape");
    let mut invalid_project = project.clone();
    invalid_project["name"] = json!("");
    let mut unserializable_project = project.clone();
    unserializable_project["opaque"] = serde_json::Value::Null;
    let mut overflowing_project = project.clone();
    overflowing_project["opaque"] = json!(u64::MAX);
    for input in [
        json!({"issues":[invalid_issue]}),
        json!({"issues":[issue.clone()],"projects":[invalid_project]}),
        json!({"issues":[issue.clone()],"projects":[unserializable_project]}),
        json!({"issues":[issue.clone()],"projects":[overflowing_project]}),
        json!({"issues":[issue.clone()],"labels":[{"id":"one","name":"One","body":"x".repeat(1024*1024)},{"id":"two","name":"Two","body":"x".repeat(1024*1024)}]}),
        json!({"agent_sessions":[{"id":"a/b","title":"unsafe"}]}),
        json!({"agent_sessions":[{"id":"a_b","title":"one"},{"id":"ab","title":"two"}]}),
        json!({"agent_sessions":[{"id":"Alpha","title":"one"},{"id":"alpha","title":"two"}]}),
        json!({"projects":[project.clone(),project]}),
    ] {
        for dry_run in [false, true] {
            let (root, store) = target();
            fs::write(
                root.path().join("input.json"),
                serde_json::to_vec(&input).unwrap(),
            )
            .unwrap();
            let before = files(store.root());
            let mut args = vec!["import", "input.json", "--replace", "--json"];
            if dry_run {
                args.push("--dry-run");
            }
            assert!(
                compatibility_preview(&store, &serde_json::to_vec(&input).unwrap(), true).is_err()
            );
            let result = run(root.path(), &args);
            assert!(!result.status.success(), "accepted invalid import: {input}");
            assert_eq!(
                files(store.root()),
                before,
                "validation changed destination: {input}"
            );
        }
    }
}

#[test]
fn merge_and_successful_dry_run_preserve_existing_semantic_records() {
    let (root, store) = target();
    fs::write(
        store.root().join("issues/WD-1.toml"),
        "# producer comment\nkey='WD-1'\ntitle='Existing backlog'\ncreated_at='2001-01-01T00:00:00Z'\nupdated_at='2001-01-02T00:00:00Z'\n[producer]\nobserved=2001-01-03T00:00:00Z\n",
    )
    .unwrap();
    let original = store.legacy_export_document().unwrap();
    fs::write(
        root.path().join("input.json"),
        r#"{"issues":[{"key":"WD-2","title":"Second issue"}],"events":[{"kind":"incoming","producer":{"opaque":true}}]}"#,
    )
    .unwrap();
    let before = files(store.root());
    let dry_run = run(
        root.path(),
        &["import", "input.json", "--dry-run", "--json"],
    );
    assert!(dry_run.status.success());
    assert_eq!(files(store.root()), before);
    let preview = compatibility_preview(
        &store,
        &fs::read(root.path().join("input.json")).unwrap(),
        false,
    )
    .unwrap();
    assert_eq!(preview.issues, 1);
    assert_eq!(preview.events, 1);
    assert_eq!(store.legacy_export_document().unwrap(), original);
    assert_eq!(files(store.root()), before);
}

#[test]
fn unknown_aggregate_metadata_and_oversized_targets_never_change_destination() {
    for content in [
        "unexpected='must preserve'\n".to_owned(),
        "x".repeat(2 * 1024 * 1024 + 1),
    ] {
        let (root, store) = target();
        fs::write(store.root().join("projects.toml"), content).unwrap();
        fs::write(root.path().join("input.json"), r#"{"issues":[]}"#).unwrap();
        let before = files(store.root());
        assert!(compatibility_preview(&store, br#"{"issues":[]}"#, true).is_err());
        let output = run(
            root.path(),
            &["import", "input.json", "--replace", "--json"],
        );
        assert!(!output.status.success());
        assert_eq!(files(store.root()), before);
        assert!(!run(root.path(), &["export", "--json"]).status.success());
    }
}

#[test]
fn pending_restore_marker_rejects_all_legacy_store_access() {
    let (_root, store) = target();
    fs::write(store.root().join("restore.yml"), "pending: true\n").unwrap();
    let before = files(store.root());
    assert!(store.load_issues().is_err());
    assert!(store.legacy_export_document().is_err());
    assert!(
        store
            .preview_legacy_import(&json!({"issues":[]}), true)
            .is_err()
    );
    assert_eq!(files(store.root()), before);
}

#[cfg(unix)]
#[test]
fn unsafe_destination_files_fail_without_blocking_or_changing_other_records() {
    for relative in ["projects.toml", "events.jsonl", "agents/session.toml"] {
        for symlink in [false, true] {
            let (root, store) = target();
            let destination = store.root().join(relative);
            if destination.exists() {
                fs::remove_file(&destination).unwrap();
            }
            let external = root.path().join("external.data");
            fs::write(&external, "keep external bytes").unwrap();
            if symlink {
                std::os::unix::fs::symlink(&external, &destination).unwrap();
            } else {
                let path = std::ffi::CString::new(destination.to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
            fs::write(root.path().join("input.json"), r#"{"issues":[]}"#).unwrap();
            let issue = fs::read(store.root().join("issues/WD-1.toml")).unwrap();
            let config = fs::read(store.root().join("config.toml")).unwrap();
            for args in [
                vec!["import", "input.json", "--replace", "--json"],
                vec!["export", "--json"],
            ] {
                let mut child = Command::cargo_bin("workdeck")
                    .unwrap()
                    .current_dir(root.path())
                    .env("XDG_CONFIG_HOME", root.path().join("isolated-config"))
                    .args(args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
                loop {
                    if let Some(status) = child.try_wait().unwrap() {
                        assert!(!status.success());
                        break;
                    }
                    if std::time::Instant::now() >= deadline {
                        child.kill().unwrap();
                        child.wait().unwrap();
                        panic!("legacy transfer blocked reading {relative}");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            assert_eq!(
                fs::read(store.root().join("issues/WD-1.toml")).unwrap(),
                issue
            );
            assert_eq!(fs::read(store.root().join("config.toml")).unwrap(), config);
            assert_eq!(fs::read(external).unwrap(), b"keep external bytes");
            assert!(fs::symlink_metadata(destination).is_ok());
        }
    }
}

#[test]
fn corrupt_destination_is_rejected_before_any_replace_write() {
    let (root, store) = target();
    fs::write(store.root().join("projects.toml"), "broken = [").unwrap();
    fs::write(root.path().join("input.json"), r#"{"issues":[]}"#).unwrap();
    let before = files(store.root());
    assert!(compatibility_preview(&store, br#"{"issues":[]}"#, true).is_err());
    assert!(
        !run(
            root.path(),
            &["import", "input.json", "--replace", "--json"]
        )
        .status
        .success()
    );
    assert_eq!(files(store.root()), before);
}

#[test]
fn duplicate_event_keys_are_corruption_not_a_lossy_export_or_replacement() {
    let (root, store) = target();
    fs::write(
        store.root().join("events.jsonl"),
        "{\"kind\":\"original\",\"kind\":\"shadow\"}\n",
    )
    .unwrap();
    fs::write(root.path().join("input.json"), r#"{"issues":[]}"#).unwrap();
    let before = files(store.root());
    assert!(compatibility_preview(&store, br#"{"issues":[]}"#, true).is_err());
    assert!(
        !run(root.path(), &["export", "--json"]).status.success(),
        "export silently collapsed duplicate keys"
    );
    assert!(
        !run(
            root.path(),
            &["import", "input.json", "--replace", "--json"]
        )
        .status
        .success()
    );
    assert_eq!(files(store.root()), before);
}

#[test]
fn raw_legacy_export_preserves_all_record_metadata_times_and_events() {
    let (root, store) = target();
    let input = json!({
        "issues":[{"key":"WD-9","title":"Preserved issue","description":"Original body","status":"done","priority":"high","created_at":"2001-01-01T00:00:00Z","updated_at":"2001-01-02T00:00:00Z","external":{"id":9,"flags":[true,false]}}],
        "projects":[{"id":"Project_A","name":"Project","description":"Original project","status":"paused","created_at":"2002-01-01T00:00:00Z","updated_at":"2002-01-02T00:00:00Z","metadata":{"budget":42}}],
        "cycles":[{"id":"cycle-a","name":"Cycle","starts_at":"2003-01-01","ends_at":"2003-02-01","status":"finished","created_at":"original-unknown-time","metadata":[1,2]}],
        "labels":[{"id":"label-a","name":"Label","color":"#abcdef","metadata":{"sort":7}}],
        "agent_sessions":[{"id":"session-a","title":"Session","started_at":"2004-01-01T00:00:00Z","ended_at":"2004-01-02T00:00:00Z","producer":{"id":"outside"},"touched_files":[{"path":"src/a.rs","change_type":"modified","extra":{"lines":3}}]}],
        "events":[{"kind":"producer_note","payload":{"details":null},"created_at":"2005-01-01T00:00:00Z","producer":{"id":"producer"}}]
    });
    fs::write(
        root.path().join("input.json"),
        serde_json::to_vec(&input).unwrap(),
    )
    .unwrap();
    fs::write(store.root().join("unrelated.txt"), "keep local context").unwrap();
    let config = fs::read(store.root().join("config.toml")).unwrap();
    fs::remove_file(store.root().join("issues/WD-1.toml")).unwrap();
    for (collection, directory, identity) in [
        ("issues", "issues", "key"),
        ("agent_sessions", "agents", "id"),
    ] {
        for record in input[collection].as_array().unwrap() {
            legacy_fixture::write(
                store.root(),
                &format!("{directory}/{}.toml", record[identity].as_str().unwrap()),
                record,
            );
        }
    }
    for collection in ["projects", "cycles", "labels"] {
        legacy_fixture::write(
            store.root(),
            &format!("{collection}.toml"),
            &json!({collection:input[collection]}),
        );
    }
    let events = input["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(store.root().join("events.jsonl"), format!("{events}\n")).unwrap();
    assert_eq!(fs::read(store.root().join("config.toml")).unwrap(), config);
    assert_eq!(
        fs::read_to_string(store.root().join("unrelated.txt")).unwrap(),
        "keep local context"
    );
    for format in ["--json", "--jsonl"] {
        let output = run(root.path(), &["export", format]);
        assert!(output.status.success());
        let workdeck_pm::ImportSource::Legacy(export) =
            workdeck_pm::decode_transfer(&output.stdout).unwrap()
        else {
            panic!("expected legacy export")
        };
        let actual = export.canonical_document();
        for key in ["issues", "projects", "cycles", "labels", "agent_sessions"] {
            assert_eq!(actual[key], input[key], "lost {key} in {format}");
        }
        assert!(
            actual["events"]
                .as_array()
                .unwrap()
                .contains(&input["events"][0])
        );
        let (copy, copy_store) = target();
        fs::write(copy.path().join("input.data"), output.stdout).unwrap();
        let before = files(copy_store.root());
        let preview = compatibility_preview(
            &copy_store,
            &fs::read(copy.path().join("input.data")).unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(preview.issues, 1);
        assert_eq!(preview.agent_sessions, 1);
        assert_eq!(files(copy_store.root()), before);
    }
}
