use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::{TempDir, tempdir};
use workdeck_pm::{
    ErrorCode, PmError,
    migration::{MigrationFault, MigrationPreview},
};

fn fixture() -> TempDir {
    let temp = tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success()
    );
    let source = temp.path().join(".agents/workdeck/issues");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("WD-1.toml"), "key = \"WD-1\"\ntitle = \"Preserve history\"\ndescription = \"# Scope\\nLegacy body.\\n\"\ncreated_at = \"2026-09-01T00:00:00Z\"\nupdated_at = \"2026-09-02T00:00:00Z\"\n").unwrap();
    temp
}

fn query(path: &Path, args: &[&str], success: bool) -> Value {
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(path)
        .env("XDG_CONFIG_HOME", path.join("test-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], success);
    value
}

fn preview(path: &Path) -> MigrationPreview {
    let result = query(
        path,
        &[
            "migrate",
            "legacy",
            "--plan-out",
            "migration-plan.json",
            "--json",
        ],
        true,
    );
    assert_eq!(result["result"]["complete"], true, "{result}");
    let bytes = fs::read(path.join("migration-plan.json")).unwrap();
    let saved: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved, result["result"]);
    serde_json::from_value(saved).unwrap()
}

#[test]
fn preview_is_read_only_and_plan_output_preserves_existing_files() {
    let temp = fixture();
    let source = temp.path().join(".agents/workdeck/issues/WD-1.toml");
    let original = fs::read(&source).unwrap();
    let plan = preview(temp.path());
    assert_eq!(plan.inventory.len(), 1);
    assert!(!temp.path().join(".workdeck").exists());
    assert_eq!(fs::read(&source).unwrap(), original);
    let saved = fs::read(temp.path().join("migration-plan.json")).unwrap();
    let conflict = query(
        temp.path(),
        &[
            "migrate",
            "legacy",
            "--plan-out",
            "migration-plan.json",
            "--json",
        ],
        false,
    );
    assert_eq!(conflict["error"]["code"], "conflict");
    assert_eq!(
        fs::read(temp.path().join("migration-plan.json")).unwrap(),
        saved
    );
    query(
        temp.path(),
        &[
            "migrate",
            "legacy",
            "--plan-out",
            ".agents/workdeck/plan.json",
            "--json",
        ],
        false,
    );
    assert!(!temp.path().join(".agents/workdeck/plan.json").exists());
}

#[test]
fn explicit_apply_preserves_legacy_and_replays_after_native_changes() {
    let temp = fixture();
    preview(temp.path());
    let args = [
        "migrate",
        "legacy",
        "--apply",
        "--plan",
        "migration-plan.json",
        "--request-id",
        "migration-once",
        "--json",
    ];
    let first = query(temp.path(), &args, true);
    let shown = query(temp.path(), &["issue", "show", "WD-1", "--json"], true);
    assert_eq!(shown["result"]["body"], "# Scope\nLegacy body.\n");
    query(
        temp.path(),
        &[
            "issue",
            "update",
            "WD-1",
            "--title",
            "Native change",
            "--json",
        ],
        true,
    );
    assert_eq!(query(temp.path(), &args, true), first);
    assert_eq!(
        query(temp.path(), &["issue", "show", "WD-1", "--json"], true)["result"]["metadata"]["title"],
        "Native change"
    );
    assert!(
        temp.path()
            .join(".agents/workdeck/issues/WD-1.toml")
            .exists()
    );
    assert!(
        !temp
            .path()
            .join(".agents/workdeck/issues/WD-2.toml")
            .exists()
    );
}

#[test]
fn interrupted_migration_has_actionable_cli_error_and_explicit_resume() {
    let temp = fixture();
    let plan = preview(temp.path());
    let request = "resume-once".parse().unwrap();
    let error = workdeck_pm::migration::apply_with_faults(&plan, &request, |point| {
        if point == MigrationFault::AfterBootstrap {
            Err(PmError::new(ErrorCode::Canceled, "injected interruption"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    let blocked = query(temp.path(), &["issue", "list", "--json"], false);
    assert_eq!(blocked["error"]["code"], "recovery_required");
    let resumed = query(
        temp.path(),
        &[
            "migrate",
            "legacy",
            "--resume",
            "--request-id",
            "resume-once",
            "--json",
        ],
        true,
    );
    assert_eq!(resumed["result"]["request_id"], "resume-once");
    query(temp.path(), &["issue", "show", "WD-1", "--json"], true);
}

#[test]
fn apply_requires_exact_plan_request_and_destination_before_writes() {
    let temp = fixture();
    let plan = preview(temp.path());
    for args in [
        vec!["migrate", "legacy", "--apply", "--json"],
        vec![
            "migrate",
            "legacy",
            "--apply",
            "--request-id",
            "x",
            "--json",
        ],
        vec![
            "migrate",
            "legacy",
            "--apply",
            "--plan",
            "migration-plan.json",
            "--request-id",
            "x",
            "--destination",
            "elsewhere",
            "--json",
        ],
    ] {
        assert_eq!(
            query(temp.path(), &args, false)["error"]["code"],
            "invalid_input"
        );
        assert!(!plan.destination_root.exists());
    }
    fs::write(plan.source_root.join("issues/WD-1.toml"), "external change").unwrap();
    query(
        temp.path(),
        &[
            "migrate",
            "legacy",
            "--apply",
            "--plan",
            "migration-plan.json",
            "--request-id",
            "changed",
            "--json",
        ],
        false,
    );
    assert!(!plan.destination_root.join("config.yml").exists());
    assert_eq!(
        fs::read_to_string(plan.source_root.join("issues/WD-1.toml")).unwrap(),
        "external change"
    );
}

#[test]
fn blocked_preview_exposes_missing_history_without_inventing_timestamps() {
    let temp = fixture();
    fs::write(
        temp.path().join(".agents/workdeck/issues/WD-2.toml"),
        "key = \"WD-2\"\ntitle = \"Unknown dates\"\n",
    )
    .unwrap();
    let value = query(
        temp.path(),
        &["migrate", "legacy", "--dry-run", "--json"],
        true,
    );
    assert_eq!(value["result"]["complete"], false);
    assert!(!value["result"]["blockers"].as_array().unwrap().is_empty());
    assert!(!temp.path().join(".workdeck").exists());
}
