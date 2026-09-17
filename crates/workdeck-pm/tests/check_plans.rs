use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
use tempfile::TempDir;
use workdeck_pm::*;
fn setup() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/code.txt"), b"first\0binary").unwrap();
    fs::write(temp.path().join("lockfile"), "dependency=1").unwrap();
    fs::create_dir(repo.root().join("commands")).unwrap();
    let command = json!({"schema":1,"repository":repo.identity(),"id":"echo","name":"Echo",
        "recipe":{"kind":"argv","argv":[{"kind":"literal","value":"sh"},{"kind":"parameter","name":"message"},{"kind":"artifact","id":"report"}]},
        "parameters":{"message":{"value_type":{"kind":"string","max_bytes":4096},"default":"hello"}},
        "cwd":".","tools":[{"name":"sh","executable":"/bin/sh"}],
        "inputs":{"trees":["src"],"dependency_files":["lockfile"]},
        "artifacts":[{"id":"report","name":"report.xml","max_bytes":4096,"required":true}]});
    write(&repo, "commands/echo.yml", &command);
    (temp, repo)
}
fn write(repo: &Repository, path: &str, value: &Value) {
    let path = repo.root().join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_yaml_ng::to_string(value).unwrap()).unwrap();
}
fn request() -> CommandPlanRequest {
    CommandPlanRequest {
        command: "echo".into(),
        arguments: BTreeMap::new(),
    }
}
#[test]
fn plan_preserves_literal_arguments_and_artifact_tokens_without_execution() {
    let (temp, repo) = setup();
    let mut req = request();
    req.arguments
        .insert("message".into(), json!("$(touch INJECTION); spaces 日本語"));
    let plan = repo.command_plan(&req).unwrap();
    assert_eq!(plan.basis, VerificationBasis::LocalFeedback);
    assert_eq!(
        plan.invocations[0].args[0],
        PlannedArgument::Literal {
            value: "$(touch INJECTION); spaces 日本語".into()
        }
    );
    assert_eq!(
        plan.invocations[0].args[1],
        PlannedArgument::ArtifactPath {
            artifact: "report".into()
        }
    );
    assert!(plan.blockers.is_empty());
    assert!(!temp.path().join("INJECTION").exists());
    assert_eq!(repo.command_plan(&req).unwrap(), plan);
    req.arguments.insert("misspelled".into(), json!(true));
    assert_eq!(
        repo.command_plan(&req).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}
#[test]
fn manifest_binds_binary_bytes_directory_membership_and_dependencies() {
    let (temp, repo) = setup();
    let first = repo.command_plan(&request()).unwrap();
    assert!(first.invocations[0].inputs.complete);
    assert!(first.invocations[0].inputs.entries.iter().any(|e|matches!(e,InputEntry::File{path,content,..} if path==std::path::Path::new("src/code.txt")&&content==&ContentHash::of(b"first\0binary"))));
    fs::write(temp.path().join("src/new.txt"), "new").unwrap();
    let second = repo.command_plan(&request()).unwrap();
    assert_ne!(first.subject, second.subject);
    fs::remove_file(temp.path().join("src/new.txt")).unwrap();
    fs::write(temp.path().join("lockfile"), "dependency=2").unwrap();
    let third = repo.command_plan(&request()).unwrap();
    assert_ne!(first.fingerprint, third.fingerprint);
}
#[test]
fn requested_and_required_profiles_remain_selected_when_impact_is_uncertain() {
    let (_temp, repo) = setup();
    for id in ["unit", "lint"] {
        write(
            &repo,
            &format!("checks/{id}.yml"),
            &json!({"schema":1,"repository":repo.identity(),"id":id,"name":id,"command":"echo","expectation":{"kind":"process","allowed_exit_codes":[0]},"affected":[format!("{id}-only")]}),
        );
    }
    write(
        &repo,
        "check-profiles/quick.yml",
        &json!({"schema":1,"repository":repo.identity(),"id":"quick","name":"Quick","checks":["unit","lint"]}),
    );
    let mut config: Config =
        serde_yaml_ng::from_slice(&fs::read(repo.root().join("config.yml")).unwrap()).unwrap();
    config.acceptance.required_profiles = vec!["quick".into()];
    fs::write(
        repo.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    let plan = repo
        .check_plan(&CheckPlanRequest {
            changed_paths: vec!["unit-only".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(plan.checks.len(), 2);
    assert!(
        plan.selection
            .iter()
            .any(|r| r.reason_code == "required_profile")
    );
    assert!(
        plan.selection
            .iter()
            .any(|r| r.reason_code == "impact_incomplete")
    );
}

fn edit_command(repo: &Repository, edit: impl FnOnce(&mut Value)) {
    let path = repo.root().join("commands/echo.yml");
    let mut value: Value = serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut value);
    write(repo, "commands/echo.yml", &value);
}

#[test]
fn shell_parameters_are_positional_and_never_interpolated_into_script() {
    let (temp, repo) = setup();
    edit_command(
        &repo,
        |v| v["recipe"] = json!({"kind":"shell","interpreter":"sh","script":"printf '%s' \"$1\"","args":[{"kind":"parameter","name":"message"}]}),
    );
    let mut req = request();
    req.arguments
        .insert("message".into(), json!("$(touch INJECTED) ; ' \" café"));
    let plan = repo.command_plan(&req).unwrap();
    assert_eq!(
        plan.invocations[0].args,
        vec![
            PlannedArgument::Literal { value: "-c".into() },
            PlannedArgument::Literal {
                value: "printf '%s' \"$1\"".into()
            },
            PlannedArgument::Literal {
                value: "workdeck:echo".into()
            },
            PlannedArgument::Literal {
                value: "$(touch INJECTED) ; ' \" café".into()
            }
        ]
    );
    assert!(!temp.path().join("INJECTED").exists());
    plan.validate().unwrap();
}

#[test]
fn source_and_definition_edits_invalidate_reviewed_plan() {
    let (temp, repo) = setup();
    let plan = repo.command_plan(&request()).unwrap();
    fs::write(temp.path().join("src/code.txt"), "new bytes").unwrap();
    assert_eq!(
        repo.revalidate_check_plan(&plan).unwrap_err().code,
        ErrorCode::StaleSource
    );
    let plan = repo.command_plan(&request()).unwrap();
    edit_command(&repo, |v| {
        v["bounds"] = json!({"timeout_seconds":1,"stdout_bytes":4096,"stderr_bytes":4096})
    });
    assert_eq!(
        repo.revalidate_check_plan(&plan).unwrap_err().code,
        ErrorCode::StaleSource
    );
    // Historical proof still validates its original source rather than rereading today.
    plan.validate().unwrap();
}

#[test]
fn input_races_and_capacity_overflow_never_return_partial_plans() {
    let (temp, repo) = setup();
    let error = repo
        .command_plan_with_limits(&request(), &InputLimits::default(), |_| {
            fs::write(temp.path().join("src/code.txt"), "edited during capture").unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    let error = repo
        .command_plan_with_limits(
            &request(),
            &InputLimits {
                max_file_bytes: 4,
                ..Default::default()
            },
            |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let error = repo
        .command_plan_with_limits(
            &request(),
            &InputLimits {
                max_entries: 1,
                ..Default::default()
            },
            |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(!repo.root().join("runs").exists());
}

#[test]
fn tool_bytes_count_toward_complete_capture_budget() {
    let (_temp, repo) = setup();
    let result = repo.command_plan_with_limits(
        &request(),
        &InputLimits {
            max_total_bytes: 64,
            ..Default::default()
        },
        |_| Ok(()),
    );
    assert!(
        result.is_err(),
        "selected executable bytes must count toward the total input bound"
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
}

#[test]
fn tree_selectors_require_directories() {
    let (_temp, repo) = setup();
    edit_command(&repo, |v| v["inputs"] = json!({"trees":["lockfile"]}));
    assert!(
        repo.command_plan(&request()).is_err(),
        "a tree selector cannot masquerade as a single-file capture"
    );
}

#[test]
fn empty_execution_plans_report_blockers_without_manufacturing_success() {
    let temp = TempDir::new().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let empty = repo.check_plan(&CheckPlanRequest::default()).unwrap();
    assert!(empty.invocations.is_empty());
    assert!(
        empty
            .blockers
            .iter()
            .any(|b| b.reason_code == "no_checks_selected")
    );
    assert_eq!(
        repo.revalidate_check_plan(&empty).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
}

#[test]
fn overlarge_execution_plans_report_blockers_but_definitions_remain_readable() {
    let (_temp, repo) = setup();
    edit_command(&repo, |v| {
        v["artifacts"] = json!([
            {"id":"report","name":"report.xml","max_bytes":32*1024*1024,"required":true},
            {"id":"second","name":"second.log","max_bytes":32*1024*1024,"required":false}
        ])
    });
    assert!(
        repo.command_catalog().is_ok(),
        "inert definitions remain readable"
    );
    let plan = repo.command_plan(&request()).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.reason_code == "execution_limits")
    );
    plan.validate().unwrap();
    assert_eq!(
        repo.revalidate_check_plan(&plan).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
}

fn canonical_hash(value: &Value) -> ContentHash {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(values) => {
                let values = values.iter().collect::<BTreeMap<_, _>>();
                Value::Object(
                    values
                        .into_iter()
                        .map(|(k, v)| (k.clone(), sorted(v)))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    ContentHash::of(&serde_json::to_vec(&sorted(value)).unwrap())
}
fn refresh_hash(value: &mut Value) {
    let mut body = value.clone();
    body.as_object_mut().unwrap().remove("fingerprint");
    value["fingerprint"] = serde_json::to_value(canonical_hash(&body)).unwrap();
}
#[test]
fn historical_manifest_proof_rejects_missing_selected_bytes_even_with_repaired_hashes() {
    let (_temp, repo) = setup();
    let plan = repo.command_plan(&request()).unwrap();
    for missing in ["lockfile", "src/code.txt"] {
        let mut value = serde_json::to_value(&plan).unwrap();
        let manifest = &mut value["invocations"][0]["inputs"];
        let entries = manifest["entries"].as_array_mut().unwrap();
        let index = entries.iter().position(|e| e["path"] == missing).unwrap();
        let removed = entries.remove(index);
        manifest["total_bytes"] =
            json!(manifest["total_bytes"].as_u64().unwrap() - removed["size"].as_u64().unwrap());
        refresh_hash(manifest);
        refresh_hash(&mut value["invocations"][0]);
        value["subject"]["content"] = serde_json::to_value(canonical_hash(&json!([[
            value["invocations"][0]["id"],
            value["invocations"][0]["inputs"]["fingerprint"]
        ]])))
        .unwrap();
        refresh_hash(&mut value);
        let corrupt: CheckPlan = serde_json::from_value(value).unwrap();
        assert!(
            corrupt.validate().is_err(),
            "missing {missing} must invalidate the complete manifest claim"
        );
    }
}

#[test]
fn broad_selection_excludes_declared_engine_outputs_but_keeps_build_trees() {
    let (temp, repo) = setup();
    edit_command(&repo, |v| v["inputs"] = json!({"trees":["."]}));
    let first = repo.command_plan(&request()).unwrap();
    first.validate().unwrap();
    for path in ["operations/fake.yml", "runs/intent.yml", ".local/log.txt"] {
        let path = repo.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "engine output").unwrap();
    }
    let second = repo.command_plan(&request()).unwrap();
    assert_eq!(first, second);
    assert!(
        first.invocations[0]
            .inputs
            .excluded_internal_paths
            .contains(&".workdeck/runs".into())
    );
    fs::create_dir(temp.path().join("target")).unwrap();
    fs::write(temp.path().join("target/generated.txt"), "build bytes").unwrap();
    let third = repo.command_plan(&request()).unwrap();
    assert_ne!(first.subject, third.subject);
    assert!(third.invocations[0].inputs.entries.iter().any(|e|matches!(e,InputEntry::File{path,..}if path==std::path::Path::new("target/generated.txt"))));
    edit_command(&repo, |v| v["inputs"] = json!({"trees":[".workdeck/runs"]}));
    assert_eq!(
        repo.command_plan(&request()).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn unbound_source_and_unavailable_prerequisites_are_explicit() {
    let (temp, repo) = setup();
    edit_command(&repo, |v| {
        v["tools"][0]["executable"] = json!("definitely-missing-pm08-tool");
        v["environment"] = json!({"TOKEN":{"kind":"inherit","source":"WORKDECK_PM08_MISSING_PREREQUISITE_TOKEN","required":true}});
    });
    let plan = repo.command_plan(&request()).unwrap();
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.reason_code == "tool_missing")
    );
    assert!(
        plan.blockers
            .iter()
            .any(|b| b.reason_code == "environment_missing")
    );
    assert_eq!(
        repo.revalidate_check_plan(&plan).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
    fs::rename(repo.root(), temp.path().join("planning")).unwrap();
    let alternate = Repository::open_source(&temp.path().join("planning")).unwrap();
    assert_eq!(
        alternate.command_plan(&request()).unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[cfg(unix)]
fn local_tool(temp: &TempDir, repo: &Repository) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir(temp.path().join("bin")).unwrap();
    fs::write(
        temp.path().join("bin/tool"),
        "#!/bin/sh\nprintf 'not run'\n",
    )
    .unwrap();
    fs::set_permissions(
        temp.path().join("bin/tool"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    edit_command(repo, |v| v["tools"][0]["executable"] = json!("bin/tool"));
}
#[cfg(unix)]
#[test]
fn portable_manifests_match_clones_and_detect_changed_tool_bytes() {
    fn copy(from: &std::path::Path, to: &std::path::Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == ".tmp" {
                continue;
            }
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &target)
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let (temp, repo) = setup();
    local_tool(&temp, &repo);
    let original = repo.command_plan(&request()).unwrap();
    let clone = TempDir::new().unwrap();
    copy(temp.path(), clone.path());
    let other = Repository::open_source(&clone.path().join(".workdeck")).unwrap();
    let copied = other.command_plan(&request()).unwrap();
    assert_eq!(original, copied);
    other.revalidate_check_plan(&original).unwrap();
    fs::write(
        clone.path().join("bin/tool"),
        "#!/bin/sh\nprintf 'changed'\n",
    )
    .unwrap();
    assert_eq!(
        other.revalidate_check_plan(&original).unwrap_err().code,
        ErrorCode::StaleSource
    );
}

#[cfg(unix)]
#[test]
fn cwd_identity_is_rechecked_even_when_not_selected_as_an_input() {
    let (temp, repo) = setup();
    fs::create_dir(temp.path().join("build")).unwrap();
    edit_command(&repo, |v| v["cwd"] = json!("build"));
    let error = repo
        .command_plan_with_limits(&request(), &InputLimits::default(), |_| {
            fs::rename(temp.path().join("build"), temp.path().join("old-build")).unwrap();
            std::os::unix::fs::symlink("src", temp.path().join("build")).unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
}

#[cfg(unix)]
#[test]
fn unsafe_input_child() {
    let Some(root) = std::env::var_os("WORKDECK_INPUT_UNSAFE_ROOT") else {
        return;
    };
    let repo = Repository::open_source(std::path::Path::new(&root)).unwrap();
    assert_eq!(
        repo.command_plan(&request()).unwrap_err().code,
        ErrorCode::UnsafePath
    );
}
#[cfg(unix)]
#[test]
fn selected_fifo_and_symlink_are_rejected_without_blocking_or_reading_targets() {
    let (temp, repo) = setup();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(temp.path().join("src/pipe"))
            .status()
            .unwrap()
            .success()
    );
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "unsafe_input_child"])
        .env("WORKDECK_INPUT_UNSAFE_ROOT", repo.root())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("FIFO input capture blocked")
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::remove_file(temp.path().join("src/pipe")).unwrap();
    std::os::unix::fs::symlink("../lockfile", temp.path().join("src/link")).unwrap();
    assert_eq!(
        repo.command_plan(&request()).unwrap_err().code,
        ErrorCode::UnsafePath
    );
}

#[test]
fn environment_revalidation_child() {
    let Some(root) = std::env::var_os("WORKDECK_INPUT_ENV_ROOT") else {
        return;
    };
    let repo = Repository::open_source(std::path::Path::new(&root)).unwrap();
    let plan: CheckPlan = serde_json::from_slice(
        &fs::read(
            std::path::Path::new(&root)
                .parent()
                .unwrap()
                .join("saved-plan.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        repo.revalidate_check_plan(&plan).unwrap_err().code,
        ErrorCode::StaleSource
    );
}
#[test]
fn named_environment_values_are_hashed_not_exposed_and_changed_values_stale() {
    let (temp, repo) = setup();
    edit_command(&repo, |v| {
        v["environment"] = json!({"TOKEN":{"kind":"inherit","source":"HOME","required":true}})
    });
    let plan = repo.command_plan(&request()).unwrap();
    let serialized = serde_json::to_string(&plan).unwrap();
    let home = std::env::var("HOME").unwrap();
    assert!(!serialized.contains(&home));
    assert_eq!(
        plan.invocations[0].inputs.environment[0].content,
        Some(ContentHash::of(home.as_bytes()))
    );
    fs::write(temp.path().join("saved-plan.json"), serialized).unwrap();
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "environment_revalidation_child"])
            .env("WORKDECK_INPUT_ENV_ROOT", repo.root())
            .env("HOME", "different-private-value")
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}
