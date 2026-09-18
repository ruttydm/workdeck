use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use workdeck_pm::{CreateIssue, IssueRecord, Repository, RequestId};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}

fn output(root: &Path, args: &[&str]) -> Value {
    let result = run(root, args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn issue(repository: &Repository, title: &str) -> IssueRecord {
    serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new(title, "Literal café text"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap()
}

#[test]
fn default_list_preserves_archived_rows_but_explicit_active_scope_excludes_them() {
    let root = tempfile::tempdir().unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    let active = issue(&repository, "Active");
    let archived = issue(&repository, "Archived");
    repository
        .archive_issue(
            archived.metadata.id.as_str(),
            &archived.source,
            true,
            &RequestId::new(),
        )
        .unwrap();
    let legacy_default = output(root.path(), &["issue", "list", "--json"]);
    assert_eq!(legacy_default["result"].as_array().unwrap().len(), 2);
    let visible = output(
        root.path(),
        &["issue", "list", "--archive", "active", "--json"],
    );
    assert_eq!(visible["result"].as_array().unwrap().len(), 1);
    assert_eq!(
        visible["result"][0]["metadata"]["id"],
        active.metadata.id.as_str()
    );
    let hidden = output(
        root.path(),
        &["issue", "list", "--archive", "archived", "--json"],
    );
    assert_eq!(
        hidden["result"][0]["metadata"]["id"],
        archived.metadata.id.as_str()
    );
}

#[test]
fn query_text_and_sort_are_literal_and_status_aliases_share_validation() {
    let root = tempfile::tempdir().unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    let zulu = issue(&repository, "Zulu");
    let alpha = issue(&repository, "Alpha");
    let value = output(
        root.path(),
        &[
            "issue",
            "list",
            "--query",
            "CAFÉ",
            "--status",
            "todo",
            "--sort",
            "title:asc",
            "--json",
        ],
    );
    assert_eq!(
        value["result"][0]["metadata"]["id"],
        alpha.metadata.id.as_str()
    );
    assert_eq!(
        value["result"][1]["metadata"]["id"],
        zulu.metadata.id.as_str()
    );
    let none = output(root.path(), &["issue", "list", "--query", ".*", "--json"]);
    assert_eq!(none["result"], json!([]));
    let invalid = run(
        root.path(),
        &["issue", "list", "--status", "missing", "--json"],
    );
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&invalid.stdout).unwrap()["error"]["code"],
        "invalid_input"
    );
}

#[test]
fn native_query_options_never_silently_fall_through_to_legacy_readers() {
    for custom in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join(if custom {
            "backlog"
        } else {
            ".agents/workdeck"
        });
        fs::create_dir_all(source.join("issues")).unwrap();
        if custom {
            fs::create_dir(root.path().join(".workdeck")).unwrap();
            fs::write(
                root.path().join(".workdeck/config.toml"),
                "[paths]\ndata_dir='backlog'\n",
            )
            .unwrap();
        }
        let before = fs::read_dir(source.join("issues")).unwrap().count();
        for flags in [
            vec!["--archive", "all"],
            vec!["--query", "text"],
            vec!["--sort", "title:asc"],
            vec!["--milestone", "M1"],
            vec!["--target", "T1"],
            vec!["--target", "T1", "--target-match", "any"],
        ] {
            let mut args = vec!["issue", "list", "--json"];
            args.extend(flags);
            let result = run(root.path(), &args);
            assert!(!result.status.success(), "{args:?}");
            let value: Value = serde_json::from_slice(&result.stdout).unwrap();
            assert!(
                matches!(
                    value["error"]["code"].as_str(),
                    Some("legacy_store" | "not_initialized")
                ),
                "{value}"
            );
        }
        assert_eq!(fs::read_dir(source.join("issues")).unwrap().count(), before);
        assert!(!root.path().join(".workdeck/config.yml").exists());
    }
}

#[test]
fn query_schema_and_command_catalog_describe_actual_options_without_initializing() {
    let root = tempfile::tempdir().unwrap();
    let schema = output(root.path(), &["schema", "issue-query", "--json"]);
    assert_eq!(schema["result"]["additionalProperties"], false);
    assert!(schema["result"]["properties"]["archive"].is_object());
    let capabilities = output(root.path(), &["capabilities", "--json"]);
    assert_eq!(
        capabilities["result"]["features"]["shared_issue_queries"],
        false
    );
    let command = capabilities["result"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|command| command["path"] == "issue list")
        .unwrap();
    for option in [
        "query",
        "archive",
        "milestone",
        "sort",
        "target",
        "target-match",
    ] {
        assert!(
            command["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|argument| argument["long"] == option),
            "missing {option}"
        );
    }
    let result = run(
        root.path(),
        &["issue", "list", "--archive", "active", "--json"],
    );
    assert_eq!(result.status.code(), Some(3));
    assert!(!root.path().join(".workdeck").exists());
}

#[test]
fn invalid_sort_fields_and_duplicates_are_structured_query_errors() {
    let root = tempfile::tempdir().unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    let issue = issue(&repository, "Stable");
    let before = fs::read(repository.root().join(&issue.path)).unwrap();
    for sorts in [
        vec!["--sort", "title"],
        vec!["--sort", "estimate:asc"],
        vec!["--sort", "title:sideways"],
        vec!["--sort", "id:asc", "--sort", "id:desc"],
    ] {
        let mut args = vec!["issue", "list", "--json"];
        args.extend(sorts);
        let result = run(root.path(), &args);
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(
            serde_json::from_slice::<Value>(&result.stdout).unwrap()["error"]["code"],
            "invalid_input"
        );
    }
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        before
    );
}

#[test]
fn hierarchy_search_targets_have_distinct_payloads_and_real_source_previews() {
    use workdeck_cli::{
        payload::{search_target_group, search_target_payload},
        repository_panels::RepositoryPanels,
    };
    use workdeck_pm::{CreatePlanning, PlanningKind};
    use workdeck_tui::workbench::{PanelPage, PanelRequest, PanelTarget, RepositoryPanelProvider};

    let root = tempfile::tempdir().unwrap();
    git2::Repository::init(root.path()).unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    for (kind, id, fields) in [
        (PlanningKind::Initiative, "initiative", json!({})),
        (PlanningKind::Project, "project", json!({})),
        (
            PlanningKind::Milestone,
            "milestone",
            json!({"project":"project"}),
        ),
        (PlanningKind::Target, "target", json!({})),
    ] {
        let input = CreatePlanning {
            id: Some(id.into()),
            name: format!("HierarchyPreview {id}"),
            body: format!("Exact {id} Markdown"),
            fields: serde_json::from_value(fields).unwrap(),
        };
        repository
            .create_planning(kind, &input, &RequestId::new())
            .unwrap();
    }
    let provider = RepositoryPanels::new(root.path(), None, 20).unwrap();
    let (results, _) = provider
        .search_matching("HierarchyPreview", 100, |target| {
            search_target_group(target) == "issues"
        })
        .unwrap();
    for (kind, target) in [
        (
            "initiative",
            PanelTarget::Initiative {
                id: "initiative".into(),
            },
        ),
        (
            "milestone",
            PanelTarget::Milestone {
                id: "milestone".into(),
            },
        ),
        (
            "target",
            PanelTarget::Target {
                id: "target".into(),
            },
        ),
    ] {
        assert!(
            results
                .iter()
                .any(|row| search_target_payload(&row.record.target)
                    == json!({"kind":kind,"id":kind}))
        );
        let preview = provider.preview(&target).unwrap();
        assert_eq!(preview.title, format!("HierarchyPreview {kind}"));
        assert!(preview.body.contains(&format!("Exact {kind} Markdown")));
        let panel = provider
            .load(&PanelRequest {
                page: PanelPage::Search,
                directory: String::new(),
                query: format!("HierarchyPreview {kind}"),
                limit: 100,
            })
            .unwrap();
        assert!(panel.entries.iter().any(|entry| entry.target == target));
    }
    assert!(
        provider
            .preview(&PanelTarget::Milestone {
                id: "../escape".into()
            })
            .is_err()
    );
}

#[test]
fn native_cli_filters_direct_and_inherited_targets_with_shared_ordering() {
    use workdeck_pm::{CreatePlanning, PlanningKind};
    let root = tempfile::tempdir().unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    for (kind, id, fields) in [
        (PlanningKind::Target, "a", json!({})),
        (PlanningKind::Target, "b", json!({})),
        (PlanningKind::Project, "project", json!({"targets":["a"]})),
        (
            PlanningKind::Milestone,
            "milestone",
            json!({"project":"project","targets":["b"]}),
        ),
        (PlanningKind::Cycle, "cycle", json!({})),
    ] {
        repository
            .create_planning(
                kind,
                &CreatePlanning {
                    id: Some(id.into()),
                    name: id.into(),
                    body: String::new(),
                    fields: serde_json::from_value(fields).unwrap(),
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    for (title, fields) in [
        (
            "Zulu inherited",
            json!({"project":"project","milestone":"milestone","cycle":"cycle"}),
        ),
        ("Alpha direct", json!({"targets":["b"]})),
        ("Other project", json!({"project":"project"})),
    ] {
        repository
            .create_issue(
                &CreateIssue {
                    title: title.into(),
                    body: String::new(),
                    fields: serde_json::from_value(fields).unwrap(),
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    let all = output(
        root.path(),
        &[
            "issue",
            "list",
            "--project",
            "project",
            "--cycle",
            "cycle",
            "--milestone",
            "milestone",
            "--target",
            "a",
            "--target",
            "b",
            "--json",
        ],
    );
    assert_eq!(all["result"].as_array().unwrap().len(), 1);
    assert_eq!(all["result"][0]["metadata"]["title"], "Zulu inherited");
    let any = output(
        root.path(),
        &[
            "issue",
            "list",
            "--target",
            "a",
            "--target",
            "b",
            "--target-match",
            "any",
            "--sort",
            "title:asc",
            "--archive",
            "active",
            "--json",
        ],
    );
    assert_eq!(
        any["result"]
            .as_array()
            .unwrap()
            .iter()
            .map(|issue| issue["metadata"]["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Alpha direct", "Other project", "Zulu inherited"]
    );
}
