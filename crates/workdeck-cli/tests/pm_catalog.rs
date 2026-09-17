use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::TempDir;

fn query(path: &std::path::Path, args: &[&str]) -> Value {
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(path)
        .env("XDG_CONFIG_HOME", path.join("test-config"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], true);
    if args.first() == Some(&"capabilities") && value["result"]["source"]["repository"].is_string()
    {
        assert_eq!(
            value["source"]["repository"],
            value["result"]["source"]["repository"]
        );
        assert_eq!(value["source"]["root"], value["result"]["source"]["root"]);
    }
    value["result"].clone()
}

#[test]
fn fresh_agent_can_discover_current_commands_and_schema_without_initializing() {
    let temp = TempDir::new().unwrap();
    let result = query(temp.path(), &["capabilities", "--json"]);
    assert_eq!(result["source"]["state"], "not_initialized");
    assert_eq!(result["api_version"], 1);
    assert!(
        result["semantics"]["writes"]
            .as_str()
            .unwrap()
            .contains("local-only protocol receipts")
    );
    let commands = result["commands"].as_array().unwrap();
    let retained_pair = commands
        .iter()
        .find(|c| c["path"] == "ci reauthenticate-red-green")
        .unwrap();
    assert_eq!(
        retained_pair["native_planning"]["requires_initialized_source"],
        true
    );

    let create = commands
        .iter()
        .find(|command| command["path"] == "issue create")
        .unwrap();
    assert_eq!(create["native_planning"]["implemented"], true);
    let delete = commands
        .iter()
        .find(|command| command["path"] == "issue delete")
        .unwrap();
    assert_eq!(delete["native_planning"]["implemented"], true);
    for path in ["project delete", "cycle delete", "label delete"] {
        let command = commands
            .iter()
            .find(|command| command["path"] == path)
            .unwrap();
        assert!(
            command["native_planning"]["limitations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|limit| limit == "force_requires_reviewed_association_resolution")
        );
    }
    let import = commands
        .iter()
        .find(|command| command["path"] == "import")
        .unwrap();
    assert_eq!(import["native_planning"]["implemented"], true);
    assert!(
        import["native_planning"]["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|limit| limit == "native_merge_requires_same_repository_identity")
    );
    assert!(
        import["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["long"] == "expected-plan")
    );
    assert!(
        delete["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["long"] == "dry-run")
    );
    assert!(
        delete["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["long"] == "expected-preview")
    );
    assert!(
        create["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["long"] == "template")
    );
    assert!(
        commands
            .iter()
            .any(|command| command["path"] == "operation recover")
    );
    assert_eq!(result["features"]["issue_mutations"], false);
    assert!(!temp.path().join(".workdeck").exists());
    let schema = query(temp.path(), &["schema", "issue", "--json"]);
    assert_eq!(schema["$defs"]["SchemaVersion"]["const"], 1);
    assert!(schema["properties"].get("imported_completion").is_some());
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "title")
    );
    assert_eq!(schema["$defs"]["Revision"]["minimum"], 1);
    let create = query(temp.path(), &["schema", "issue-create", "--json"]);
    assert!(
        create["properties"]["fields"]["properties"]
            .get("status")
            .is_some()
    );
    assert!(
        create["properties"]["fields"]["properties"]
            .get("id")
            .is_none()
    );
    assert_eq!(
        create["properties"]["fields"]["additionalProperties"],
        false
    );
    let planning = query(temp.path(), &["schema", "planning-save", "--json"]);
    assert!(
        planning["properties"]["fields"]["properties"]
            .get("status")
            .is_some()
    );
    assert!(
        planning["properties"]["fields"]["properties"]
            .get("revision")
            .is_none()
    );
    assert!(
        planning["properties"]["fields"]["properties"]
            .get("imported")
            .is_none()
    );
    assert_eq!(
        planning["properties"]["fields"]["additionalProperties"],
        false
    );
    assert!(!temp.path().join(".workdeck").exists());
}

#[test]
fn initialized_capabilities_bind_local_checks_without_claiming_ci_admission() {
    let temp = TempDir::new().unwrap();
    let repository = workdeck_pm::Repository::init(temp.path(), "WD").unwrap();
    let result = query(temp.path(), &["capabilities", "--json"]);
    assert_eq!(result["source"]["state"], "ready");
    assert_eq!(
        result["source"]["repository"],
        repository.identity().as_str()
    );
    assert_eq!(result["features"]["issue_mutations"], true);
    assert_eq!(result["semantics"]["native_mutations"], "qualified");
    assert_eq!(result["features"]["check_execution"], cfg!(unix));
    assert_eq!(result["features"]["shared_claims"], cfg!(unix));
    assert_eq!(result["features"]["ci_admission"], false);
    assert!(
        result["schemas"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "issue-mutation")
    );
}

#[test]
fn generated_pm_protocol_matches_installed_catalog_without_initializing_a_repository() {
    let root = TempDir::new().unwrap();
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (kind, path) in [
        ("skill", "skills/workdeck-pm/SKILL.md"),
        ("commands", "docs/reference/workdeck-pm-commands.md"),
        ("schemas", "docs/reference/workdeck-pm-schemas.json"),
    ] {
        let output = Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root.path())
            .args(["protocol", "render", kind])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let checked = std::fs::read(repository.join(path))
            .unwrap_or_else(|error| panic!("missing generated {path}: {error}"));
        assert_eq!(
            output.stdout, checked,
            "{path} differs from actual installed catalog; regenerate via workdeck protocol render {kind}"
        );
    }
    assert!(!root.path().join(".workdeck").exists());
    assert!(!root.path().join("AGENTS.md").exists());
}

#[test]
fn unavailable_source_capabilities_bound_diagnostics_and_keep_recovery_guidance() {
    let root = TempDir::new().unwrap();
    let repo = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let path = repo.root().join("config.yml");
    let original = std::fs::read_to_string(&path).unwrap();
    assert!(original.contains("initial: ready"));
    std::fs::write(
        &path,
        original.replace(
            "initial: ready",
            &format!("initial: {}", "z".repeat(50_000)),
        ),
    )
    .unwrap();
    let value = query(root.path(), &["capabilities", "--json"]);
    assert_eq!(value["source"]["state"], "invalid_schema");
    assert!(
        serde_json::to_vec(&value["source"]["diagnostic"])
            .unwrap()
            .len()
            <= 16 * 1024
    );
    assert_eq!(value["source"]["diagnostic_truncated"], true);
    assert_eq!(value["source"]["retryable"], false);
    assert!(value["source"]["recovery_actions"].is_array());
    assert_eq!(value["features"]["task_context"], false);
}

#[test]
fn fresh_agent_can_request_small_capability_projection_without_receiving_the_full_catalog() {
    let root = TempDir::new().unwrap();
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .args([
            "capabilities",
            "--fields",
            "source.state,features.task_context,command_version",
            "--compact",
            "--no-input",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() < 1024);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["source"]["state"], "not_initialized");
    assert_eq!(value["result"]["features"]["task_context"], false);
    assert_eq!(value["result"]["command_version"], 1);
    assert!(value["result"].get("commands").is_none());
    assert!(!root.path().join(".workdeck").exists());
}

#[test]
fn execution_definition_schemas_match_strict_extension_namespaces() {
    let temp = TempDir::new().unwrap();
    for name in ["command-definition", "check-definition", "check-profile"] {
        let schema = query(temp.path(), &["schema", name, "--json"]);
        assert_eq!(schema["additionalProperties"], false, "{name}");
        assert_eq!(
            schema["patternProperties"]["^x-[a-z0-9][a-z0-9_-]{0,95}$"],
            true
        );
        assert!(schema["properties"].get("custom").is_some());
    }
    let plan = query(temp.path(), &["schema", "check-plan", "--json"]);
    for name in [
        "CommandDefinition",
        "CheckDefinition",
        "CheckProfileDefinition",
    ] {
        assert_eq!(plan["$defs"][name]["additionalProperties"], false, "{name}");
    }
    assert!(!temp.path().join(".workdeck").exists());
}
