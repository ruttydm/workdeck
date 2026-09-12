use serde_json::json;
use std::fs;
use workdeck_pm::{Repository, RequestId, SnapshotImportMode};

fn definitions(repo: &Repository) {
    for (path, value) in [
        (
            "commands/test.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"test","name":"Test",
            "recipe":{"kind":"argv","argv":[{"kind":"literal","value":"tool"}]},
            "cwd":".","tools":[{"name":"tool","executable":"/bin/sh"}],"inputs":{"files":["source.txt"]},
            "custom":{"retained":true},"x-authored":{"intent":"keep exact source"}}),
        ),
        (
            "checks/unit.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit","command":"test",
            "expectation":{"kind":"process","allowed_exit_codes":[0]}}),
        ),
        (
            "check-profiles/quick.yml",
            json!({"schema":1,"repository":repo.identity(),"id":"quick","name":"Quick","checks":["unit"]}),
        ),
    ] {
        let path = repo.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                "# authored comment\n{}",
                serde_yaml_ng::to_string(&value).unwrap()
            ),
        )
        .unwrap();
    }
}

#[test]
fn doctor_inspects_execution_definitions_instead_of_ignoring_malformed_namespaces() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    fs::create_dir(repo.root().join("commands")).unwrap();
    fs::write(repo.root().join("commands/broken.yml"), "schema: 99\n").unwrap();
    let report = repo.doctor().unwrap();
    assert!(!report.valid, "doctor must inspect execution definitions");
    assert!(report.errors.iter().any(|error| {
        error
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("commands/broken.yml"))
    }));
}

#[test]
fn definitions_survive_native_export_and_exact_import_with_their_authored_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    definitions(&repo);
    let snapshot = repo.export_snapshot().unwrap();
    let original = fs::read(repo.root().join("commands/test.yml")).unwrap();
    fs::write(
        repo.root().join("commands/test.yml"),
        String::from_utf8(original.clone())
            .unwrap()
            .replace("name: Test", "name: Edited"),
    )
    .unwrap();
    let plan = repo
        .preview_snapshot_import(&snapshot, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(plan.allowed, "{plan:?}");
    repo.import_snapshot(
        &snapshot,
        SnapshotImportMode::ReplaceMatching,
        Some(&plan.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.root().join("commands/test.yml")).unwrap(),
        original
    );
    assert_eq!(repo.command_catalog().unwrap().checks.len(), 1);
    assert!(repo.doctor().unwrap().valid);
}

#[test]
fn saved_plans_are_disposable_exact_and_never_execute_recipes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    definitions(&repo);
    fs::write(temp.path().join("source.txt"), "original").unwrap();
    let plan = repo
        .command_plan(&workdeck_pm::CommandPlanRequest {
            command: "test".into(),
            arguments: Default::default(),
        })
        .unwrap();
    let path = repo.save_check_plan(&plan).unwrap();
    assert_eq!(repo.save_check_plan(&plan).unwrap(), path);
    assert_eq!(repo.load_check_plan(&plan.fingerprint).unwrap(), plan);
    assert!(!repo.root().join("runs").exists());
    assert_eq!(
        fs::read(repo.root().join(".local/plans/.gitignore")).unwrap(),
        b"*\n"
    );
    assert!(
        !repo
            .export_snapshot()
            .unwrap()
            .files
            .iter()
            .any(|file| file.path.starts_with(".local"))
    );
    fs::write(temp.path().join("source.txt"), "changed").unwrap();
    assert_eq!(
        repo.load_check_plan(&plan.fingerprint).unwrap(),
        plan,
        "load retains historical plan; run must check freshness"
    );
    fs::write(&path, "{}").unwrap();
    assert!(repo.load_check_plan(&plan.fingerprint).is_err());
    assert!(repo.save_check_plan(&plan).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{}");
    let other = tempfile::tempdir().unwrap();
    let foreign = Repository::init(other.path(), "WD").unwrap();
    assert!(foreign.save_check_plan(&plan).is_err());
}

#[test]
fn doctor_and_export_reject_unproven_execution_history() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let run = workdeck_pm::LocalRunId::new();
    let path = repo.root().join("runs").join(run.as_str());
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("intent.yml"), "schema: 1\nstate: passed\n").unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.export_snapshot().is_err());
}

#[test]
#[cfg(unix)]
fn canonical_run_history_exports_with_receipts_but_local_logs_do_not() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    definitions(&repo);
    fs::write(temp.path().join("source.txt"), "original").unwrap();
    let path = repo.root().join("commands/test.yml");
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    command["recipe"]["argv"] = json!([
        {"kind":"literal","value":"tool"},
        {"kind":"literal","value":"-c"},
        {"kind":"literal","value":"printf retained-output"}
    ]);
    fs::write(&path, serde_yaml_ng::to_string(&command).unwrap()).unwrap();
    let plan = repo
        .command_plan(&workdeck_pm::CommandPlanRequest {
            command: "test".into(),
            arguments: Default::default(),
        })
        .unwrap();
    let result = repo
        .run_check_plan(
            &workdeck_pm::CheckRunRequest {
                expected_plan: plan.fingerprint.clone(),
                plan,
                actor: "test-agent".into(),
            },
            &RequestId::new(),
            &Default::default(),
        )
        .unwrap();
    assert_eq!(result.state, workdeck_pm::RunState::Passed);
    let snapshot = repo.export_snapshot().unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == result.run.path)
    );
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.path == result.results.as_ref().unwrap().path)
    );
    assert!(
        !snapshot
            .files
            .iter()
            .any(|file| file.path.starts_with(".local"))
    );
    let preview = repo
        .preview_snapshot_import(&snapshot, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(preview.allowed, "{preview:?}");
    let report = repo.doctor().unwrap();
    assert!(report.valid, "{report:?}");
    // Publication proof is necessary even when the result's own schema remains valid.
    let finished = result
        .receipts
        .iter()
        .find(|receipt| receipt.operation == "execution.finish")
        .unwrap();
    fs::remove_file(
        repo.root()
            .join("operations")
            .join(format!("{}.yml", finished.operation_id)),
    )
    .unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.check_status(&result.run.intent.id).is_err());
    assert!(repo.export_snapshot().is_err());
}
