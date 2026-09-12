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
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    value
}
fn files(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn walk(root: &Path, current: &Path, output: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                output.insert(relative, None);
                walk(root, &path, output);
            } else {
                output.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    let mut result = BTreeMap::new();
    walk(root, root, &mut result);
    result
}

#[test]
fn initiative_milestone_and_target_commands_create_show_list_update_archive_and_replay() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    success(
        root,
        &[
            "project",
            "create",
            "Owning project",
            "--id",
            "project",
            "--json",
        ],
    );
    for (kind, prefix) in [
        ("initiative", "INI-"),
        ("milestone", "MIL-"),
        ("target", "TGT-"),
    ] {
        let mut args = vec![
            kind,
            "create",
            "A declared outcome",
            "--request-id",
            kind,
            "--json",
        ];
        if kind == "milestone" {
            args.extend(["--project", "project"]);
        }
        let created = success(root, &args);
        let id = created["result"]["metadata"]["id"].as_str().unwrap();
        assert!(id.starts_with(prefix), "{kind}: {id}");
        assert!(
            root.join(format!(".workdeck/{kind}s/{id}/item.md"))
                .exists()
        );
        let revision = created["result"]["source"]["revision"].to_string();
        let content = created["result"]["source"]["content"].as_str().unwrap();
        let updated = success(
            root,
            &[
                kind,
                "update",
                id,
                "--name",
                "Renamed outcome",
                "--outcome",
                "accepted=Declared acceptance",
                "--expected-revision",
                &revision,
                "--expected-content",
                content,
                "--json",
            ],
        );
        assert_eq!(
            updated["result"]["metadata"]["outcomes"],
            json!([{"id":"accepted","description":"Declared acceptance"}])
        );
        assert_eq!(success(root, &args)["result"], created["result"]);
        let before = files(&root.join(".workdeck"));
        let stale = run(
            root,
            &[
                kind,
                "update",
                id,
                "--name",
                "Stale",
                "--expected-revision",
                &revision,
                "--expected-content",
                content,
                "--json",
            ],
        );
        assert_eq!(stale.status.code(), Some(4));
        assert_eq!(
            serde_json::from_slice::<Value>(&stale.stdout).unwrap()["error"]["code"],
            "stale_source"
        );
        assert_eq!(files(&root.join(".workdeck")), before);
        assert_eq!(
            success(root, &[kind, "show", id, "--json"])["result"],
            updated["result"]
        );
        assert!(
            success(root, &[kind, "list", "--json"])["result"]
                .as_array()
                .unwrap()
                .iter()
                .any(|record| record["metadata"]["id"] == id)
        );
        assert_eq!(
            success(root, &[kind, "archive", id, "--json"])["result"]["metadata"]["archived"],
            true
        );
        assert_eq!(
            success(root, &[kind, "archive", id, "--restore", "--json"])["result"]["metadata"]["archived"],
            false
        );
    }
}

#[test]
fn project_fields_dates_criteria_and_explicit_clear_use_shared_validation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repository = workdeck_pm::Repository::init(root, "WD").unwrap();
    success(
        root,
        &[
            "initiative",
            "create",
            "Outcome",
            "--id",
            "initiative",
            "--json",
        ],
    );
    success(
        root,
        &["target", "create", "Release", "--id", "release", "--json"],
    );
    let project = success(
        root,
        &[
            "project",
            "create",
            "Project",
            "--id",
            "project",
            "--initiative",
            "initiative",
            "--lead",
            "local",
            "--scope",
            "Bounded scope",
            "--goal",
            "Deliver value",
            "--starts-at",
            "2026-09-01",
            "--ends-at",
            "2026-09-30",
            "--target",
            "release",
            "--exit-criterion",
            "accepted=Review complete",
            "--json",
        ],
    );
    assert_eq!(project["result"]["metadata"]["initiative"], "initiative");
    assert_eq!(project["result"]["metadata"]["targets"], json!(["release"]));
    assert_eq!(
        project["result"]["metadata"]["exit_criteria"],
        json!([{"id":"accepted","description":"Review complete"}])
    );
    let cleared = success(
        root,
        &[
            "project", "update", "project", "--clear", "lead", "--clear", "targets", "--json",
        ],
    );
    assert!(cleared["result"]["metadata"].get("lead").is_none());
    assert!(cleared["result"]["metadata"].get("targets").is_none());
    let before = files(repository.root());
    for args in [
        vec!["project", "update", "project", "--starts-at", "2026-10-01"],
        vec![
            "project", "update", "project", "--lead", "local", "--clear", "lead",
        ],
        vec!["project", "update", "project", "--clear", "id"],
        vec![
            "project",
            "update",
            "project",
            "--exit-criterion",
            "missing-description",
        ],
        vec!["milestone", "create", "Missing parent"],
        vec![
            "milestone",
            "create",
            "Unknown parent",
            "--project",
            "missing",
        ],
    ] {
        let mut args = args;
        args.push("--json");
        assert!(!run(root, &args).status.success(), "{args:?}");
        assert_eq!(files(repository.root()), before, "{args:?}");
    }
}

#[test]
fn new_planning_commands_never_fall_through_to_legacy_or_initialize_on_reads() {
    for source in ["fresh", "legacy", "custom"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        if source != "fresh" {
            let legacy = if source == "custom" {
                "planning-data"
            } else {
                ".agents/workdeck"
            };
            fs::create_dir_all(root.join(legacy).join("issues")).unwrap();
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
        for kind in ["initiative", "milestone", "target"] {
            for args in [
                vec![kind, "list", "--json"],
                vec![kind, "create", "No authority", "--json"],
            ] {
                let output = run(root, &args);
                assert!(!output.status.success(), "{args:?}");
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert!(
                    matches!(
                        value["error"]["code"].as_str(),
                        Some("legacy_store" | "not_initialized")
                    ),
                    "{value}"
                );
                assert_eq!(files(root), before);
            }
        }
    }
}

#[test]
fn cycle_goal_scope_lead_and_dates_are_editable_without_rewriting_identity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    let created = success(
        root,
        &[
            "cycle",
            "create",
            "September",
            "--id",
            "september",
            "--lead",
            "local",
            "--scope",
            "Deliver the cycle",
            "--goal",
            "A useful increment",
            "--starts-at",
            "2026-09-01",
            "--ends-at",
            "2026-09-30",
            "--json",
        ],
    );
    let metadata = &created["result"]["metadata"];
    assert_eq!(metadata["lead"], "local");
    assert_eq!(metadata["scope"], "Deliver the cycle");
    assert_eq!(metadata["goal"], "A useful increment");
    let updated = success(
        root,
        &[
            "cycle",
            "update",
            "september",
            "--goal",
            "Revised goal",
            "--clear",
            "lead",
            "--json",
        ],
    );
    assert_eq!(updated["result"]["metadata"]["id"], metadata["id"]);
    assert_eq!(
        updated["result"]["metadata"]["created_at"],
        metadata["created_at"]
    );
    assert_eq!(
        updated["result"]["metadata"]["starts_at"],
        metadata["starts_at"]
    );
    assert_eq!(updated["result"]["metadata"]["scope"], metadata["scope"]);
    assert_eq!(updated["result"]["metadata"]["goal"], "Revised goal");
    assert!(updated["result"]["metadata"].get("lead").is_none());
}

#[test]
fn issue_association_flags_override_defaults_clear_explicitly_and_replay_after_reassignment() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    for (kind, id) in [
        ("project", "project"),
        ("project", "other"),
        ("cycle", "cycle"),
        ("target", "delivery"),
        ("target", "release"),
    ] {
        success(root, &[kind, "create", id, "--id", id, "--json"]);
    }
    success(
        root,
        &[
            "milestone",
            "create",
            "Outcome",
            "--id",
            "milestone",
            "--project",
            "project",
            "--json",
        ],
    );
    fs::write(root.join("issue.json"), r#"{"title":"Associations","fields":{"project":"missing","cycle":"missing","milestone":"missing","targets":["missing"]}}"#).unwrap();
    let args = [
        "issue",
        "create",
        "--from-json",
        "issue.json",
        "--project",
        "project",
        "--cycle",
        "cycle",
        "--milestone",
        "milestone",
        "--target",
        "delivery",
        "--target",
        "release",
        "--request-id",
        "associations",
        "--json",
    ];
    let created = success(root, &args);
    let id = created["result"]["metadata"]["id"].as_str().unwrap();
    assert_eq!(created["result"]["metadata"]["milestone"], "milestone");
    assert_eq!(
        created["result"]["metadata"]["targets"],
        json!(["delivery", "release"])
    );
    let before = files(&root.join(".workdeck"));
    for args in [
        vec!["issue", "update", id, "--milestone", "missing"],
        vec!["issue", "update", id, "--target", "missing"],
        vec!["issue", "update", id, "--project", "other"],
        vec![
            "issue",
            "update",
            id,
            "--milestone",
            "milestone",
            "--clear",
            "milestone",
        ],
        vec![
            "issue", "update", id, "--target", "delivery", "--clear", "targets",
        ],
        vec![
            "issue",
            "update",
            id,
            "--project",
            "project",
            "--clear",
            "project",
        ],
        vec![
            "issue", "update", id, "--cycle", "cycle", "--clear", "cycle",
        ],
        vec!["issue", "update", id, "--clear", "id"],
        vec![
            "issue", "update", id, "--clear", "cycle", "--clear", "cycle",
        ],
    ] {
        let mut args = args;
        args.push("--json");
        assert!(!run(root, &args).status.success(), "{args:?}");
        assert_eq!(files(&root.join(".workdeck")), before, "{args:?}");
    }
    let changed = success(
        root,
        &[
            "issue",
            "update",
            id,
            "--project",
            "other",
            "--clear",
            "milestone",
            "--unset",
            "targets",
            "--clear",
            "cycle",
            "--json",
        ],
    );
    assert_eq!(changed["result"]["metadata"]["project"], "other");
    for field in ["milestone", "targets", "cycle"] {
        assert!(
            changed["result"]["metadata"].get(field).is_none(),
            "{field}"
        );
    }
    assert_eq!(success(root, &args)["result"], created["result"]);
    let cleared = success(
        root,
        &[
            "issue",
            "create",
            "--from-json",
            "issue.json",
            "--clear",
            "project",
            "--clear",
            "cycle",
            "--clear",
            "milestone",
            "--clear",
            "targets",
            "--json",
        ],
    );
    for field in ["project", "cycle", "milestone", "targets"] {
        assert!(
            cleared["result"]["metadata"].get(field).is_none(),
            "{field}"
        );
    }
    fs::create_dir_all(root.join(".workdeck/templates/issues")).unwrap();
    fs::write(root.join(".workdeck/templates/issues/associated.md"), "---\nschema: 1\nid: associated\nname: Associated issue\ndefaults:\n  project: project\n  cycle: cycle\n  milestone: milestone\n  targets: [delivery]\n---\nTemplate body.\n").unwrap();
    let inherited = success(
        root,
        &[
            "issue",
            "create",
            "Inherited",
            "--template",
            "associated",
            "--json",
        ],
    );
    assert_eq!(inherited["result"]["metadata"]["milestone"], "milestone");
    assert_eq!(
        inherited["result"]["metadata"]["targets"],
        json!(["delivery"])
    );
    let cleared = success(
        root,
        &[
            "issue",
            "create",
            "Cleared",
            "--template",
            "associated",
            "--clear",
            "project",
            "--clear",
            "cycle",
            "--clear",
            "milestone",
            "--clear",
            "targets",
            "--json",
        ],
    );
    for field in ["project", "cycle", "milestone", "targets"] {
        assert!(
            cleared["result"]["metadata"].get(field).is_none(),
            "{field}"
        );
    }
    assert_eq!(cleared["result"]["body"], "Template body.\n");
}

#[test]
fn planning_show_members_uses_shared_membership_with_explicit_archive_scope() {
    use workdeck_pm::{ArchiveFilter, PlanningKind, PlanningMembershipQuery};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repository = workdeck_pm::Repository::init(root, "WD").unwrap();
    for kind in ["initiative", "target", "cycle", "label"] {
        success(root, &[kind, "create", kind, "--id", kind, "--json"]);
    }
    success(
        root,
        &[
            "project",
            "create",
            "Project",
            "--id",
            "project",
            "--initiative",
            "initiative",
            "--target",
            "target",
            "--json",
        ],
    );
    success(
        root,
        &[
            "milestone",
            "create",
            "Milestone",
            "--id",
            "milestone",
            "--project",
            "project",
            "--json",
        ],
    );
    fs::write(root.join("issue.json"), r#"{"title":"Member","fields":{"project":"project","cycle":"cycle","milestone":"milestone","labels":["label"]}}"#).unwrap();
    let active = success(
        root,
        &["issue", "create", "--from-json", "issue.json", "--json"],
    );
    let archived = success(
        root,
        &["issue", "create", "--from-json", "issue.json", "--json"],
    );
    success(
        root,
        &[
            "issue",
            "archive",
            archived["result"]["metadata"]["id"].as_str().unwrap(),
            "--json",
        ],
    );
    success(root, &["project", "archive", "project", "--json"]);
    let before = files(repository.root());
    for (name, kind) in [
        ("initiative", PlanningKind::Initiative),
        ("project", PlanningKind::Project),
        ("milestone", PlanningKind::Milestone),
        ("cycle", PlanningKind::Cycle),
        ("target", PlanningKind::Target),
        ("label", PlanningKind::Label),
    ] {
        for (scope, archive, count) in [
            (None, ArchiveFilter::Active, 1),
            (Some("all"), ArchiveFilter::All, 2),
            (Some("archived"), ArchiveFilter::Archived, 1),
        ] {
            let mut args = vec![name, "show", name, "--members", "--json"];
            if let Some(scope) = scope {
                args.extend(["--archive", scope]);
            }
            let members = success(root, &args);
            let mut query = PlanningMembershipQuery::new(kind, name);
            query.issues.archive = archive;
            assert_eq!(
                members["result"],
                serde_json::to_value(repository.planning_membership(&query).unwrap()).unwrap()
            );
            assert_eq!(members["result"]["issues"].as_array().unwrap().len(), count);
            if archive == ArchiveFilter::Active {
                assert_eq!(
                    members["result"]["issues"][0]["metadata"]["id"],
                    active["result"]["metadata"]["id"]
                );
            }
            assert_eq!(files(repository.root()), before);
        }
        let plain = success(root, &[name, "show", name, "--json"]);
        assert_eq!(plain["result"]["metadata"]["id"], name);
        assert!(plain["result"].get("issues").is_none());
        assert!(
            !run(root, &[name, "show", name, "--archive", "all", "--json"])
                .status
                .success()
        );
    }
}

#[test]
fn project_policy_assessment_requires_members_criteria_and_attributed_acceptance() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    let created = success(
        root,
        &[
            "project",
            "create",
            "Release",
            "--id",
            "release",
            "--exit-criterion",
            "shipped=Release is shipped",
            "--json",
        ],
    );
    let before = files(&root.join(".workdeck"));
    let initial = success(root, &["project", "assess", "release", "--json"]);
    assert_eq!(initial["result"]["allowed"], false);
    assert!(
        initial["result"]["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|condition| condition["reason_code"] == "no_issue_members")
    );
    assert_eq!(files(&root.join(".workdeck")), before);

    let issue = success(
        root,
        &[
            "issue",
            "create",
            "Ship release",
            "--project",
            "release",
            "--json",
        ],
    );
    let issue_id = issue["result"]["metadata"]["id"].as_str().unwrap();
    let pending = success(root, &["project", "assess", "release", "--json"]);
    assert_eq!(pending["result"]["allowed"], false);
    success(root, &["issue", "done", issue_id, "--json"]);
    let without_acceptance = success(root, &["project", "assess", "release", "--json"]);
    assert_eq!(without_acceptance["result"]["allowed"], false);
    assert_eq!(without_acceptance["result"]["basis"], "declared");

    let shown = success(root, &["project", "show", "release", "--json"]);
    let revision = shown["result"]["source"]["revision"].to_string();
    let content = shown["result"]["source"]["content"].as_str().unwrap();
    let completed = success(
        root,
        &[
            "project",
            "complete",
            "release",
            "--actor",
            "local",
            "--reason",
            "Reviewed the shipped release",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
    );
    assert_eq!(completed["result"]["metadata"]["status"], "done");
    assert_eq!(
        completed["result"]["metadata"]["id"],
        created["result"]["metadata"]["id"]
    );
    assert!(completed["receipt"]["request_id"].is_string());
}
