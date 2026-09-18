use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;
use workdeck_pm::*;

fn setup() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    (temp, repo)
}
fn command(repo: &Repository) -> Value {
    json!({"schema":1,"repository":repo.identity(),"id":"test","name":"Tests",
        "recipe":{"kind":"argv","argv":[{"kind":"literal","value":"tool"},{"kind":"parameter","name":"message"}]},
        "parameters":{"message":{"value_type":{"kind":"string","max_bytes":4096},"default":"safe"}},
        "cwd":".","tools":[{"name":"tool","executable":"/bin/sh"}],"inputs":{"files":["source.txt"]},
        "custom":{"nested":{"number":42}},"x-owner":{"name":"test"}})
}
fn write(repo: &Repository, path: &str, value: &Value) {
    let path = repo.root().join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!(
            "# Retained author comment\n{}",
            serde_yaml_ng::to_string(value).unwrap()
        ),
    )
    .unwrap();
}
#[test]
fn discovery_is_inert_and_preserves_exact_custom_source() {
    let (temp, repo) = setup();
    let mut value = command(&repo);
    value["recipe"] =
        json!({"kind":"shell","interpreter":"tool","script":"touch MUST_NOT_EXECUTE","args":[]});
    write(&repo, "commands/test.yml", &value);
    let before = fs::read(repo.root().join("commands/test.yml")).unwrap();
    let record = repo.command("test").unwrap();
    assert_eq!(record.definition.custom["nested"]["number"], 42);
    assert_eq!(record.definition.extra["x-owner"]["name"], "test");
    assert_eq!(record.document.as_bytes(), before);
    assert_eq!(record.content, ContentHash::of(&before));
    assert!(!temp.path().join("MUST_NOT_EXECUTE").exists());
    assert_eq!(
        repo.command_catalog().unwrap(),
        repo.command_catalog().unwrap()
    );
    assert_eq!(
        fs::read(repo.root().join("commands/test.yml")).unwrap(),
        before
    );
}
#[test]
fn catalog_captures_references_and_reports_malformed_records() {
    let (_temp, repo) = setup();
    write(&repo, "commands/test.yml", &command(&repo));
    write(
        &repo,
        "checks/unit.yml",
        &json!({"schema":1,"repository":repo.identity(),"id":"unit","name":"Unit","command":"test","expectation":{"kind":"junit","artifact":"report","suites":["unit"],"minimum_tests":1,"maximum_skipped":null,"allowed_exit_codes":[0]}}),
    );
    // A report must refer to a real artifact declaration.
    let report = repo.validate_command_catalog().unwrap();
    assert!(!report.valid);
    assert!(report.errors.iter().any(|e| e.message.contains("artifact")));
    let mut valid = command(&repo);
    valid["artifacts"] =
        json!([{"id":"report","name":"report.xml","max_bytes":65536,"required":true}]);
    write(&repo, "commands/test.yml", &valid);
    write(
        &repo,
        "check-profiles/quick.yml",
        &json!({"schema":1,"repository":repo.identity(),"id":"quick","name":"Quick","checks":["unit"]}),
    );
    let catalog = repo.command_catalog().unwrap();
    assert_eq!(catalog.checks.len(), 1);
    assert_eq!(catalog.profiles.len(), 1);
    assert!(repo.validate_command_catalog().unwrap().valid);
    valid["typo"] = json!(true);
    write(&repo, "commands/test.yml", &valid);
    assert_eq!(
        repo.command_catalog().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
}
#[test]
fn unsafe_ids_unexpected_nested_records_and_schema_versions_are_explicit() {
    let (_temp, repo) = setup();
    let mut value = command(&repo);
    value["id"] = json!("../outside");
    write(&repo, "commands/test.yml", &value);
    assert_eq!(
        repo.command_catalog().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
    value = command(&repo);
    value["schema"] = json!(99);
    write(&repo, "commands/test.yml", &value);
    assert_eq!(
        repo.command_catalog().unwrap_err().code,
        ErrorCode::UnsupportedSchema
    );
    fs::remove_file(repo.root().join("commands/test.yml")).unwrap();
    write(&repo, "commands/nested/test.yml", &command(&repo));
    assert_eq!(
        repo.command_catalog().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
}
