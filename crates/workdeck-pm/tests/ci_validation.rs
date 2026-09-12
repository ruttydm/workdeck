use std::{fs, path::Path, process::Command};
use tempfile::TempDir;
use workdeck_pm::{
    CiRevision, CiValidateRequest, CreateIssue, ErrorCode, IssueRecord, Repository, RequestId,
    ci_validate,
};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn fixture() -> (TempDir, Repository, IssueRecord) {
    let directory = TempDir::new().unwrap();
    git(directory.path(), &["init", "--quiet"]);
    git(directory.path(), &["config", "user.name", "Staged Doctor"]);
    git(
        directory.path(),
        &["config", "user.email", "doctor@example.invalid"],
    );
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue = create(&repository, "Indexed issue");
    fs::write(
        directory.path().join("application.txt"),
        "application before\n",
    )
    .unwrap();
    git(
        directory.path(),
        &["add", "--", ".workdeck", "application.txt"],
    );
    git(
        directory.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "initial"],
    );
    (directory, repository, issue)
}
fn create(repository: &Repository, title: &str) -> IssueRecord {
    serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new(title, "Body"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}

fn revision(root: &Path) -> CiRevision {
    CiRevision::Commit {
        oid: String::from_utf8(git(root, &["rev-parse", "HEAD"]))
            .unwrap()
            .trim()
            .parse()
            .unwrap(),
    }
}
fn commit(root: &Path) {
    git(root, &["add", "--", ".workdeck"]);
    git(
        root,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "candidate"],
    );
}
#[test]
fn exact_commits_ignore_dirty_index_and_working_configuration() {
    let (directory, repository, _) = fixture();
    let base = revision(directory.path());
    create(&repository, "Candidate");
    commit(directory.path());
    let head = revision(directory.path());
    fs::write(repository.root().join("config.yml"), "invalid: [").unwrap();
    git(directory.path(), &["add", "--", ".workdeck/config.yml"]);
    fs::remove_file(repository.root().join("config.yml")).unwrap();
    let index = git(directory.path(), &["ls-files", "--stage"]);
    let report = ci_validate(directory.path(), &CiValidateRequest { base, head }).unwrap();
    assert!(report.valid, "{:?}", report.head_report.errors);
    assert_ne!(report.base.commit, report.head.commit);
    assert_ne!(report.base.content, report.head.content);
    assert_eq!(report.head.repository, *repository.identity());
    assert_eq!(git(directory.path(), &["ls-files", "--stage"]), index);
    assert!(!repository.root().join("config.yml").exists());
}
#[test]
fn broken_candidate_is_not_hidden_by_repaired_working_tree() {
    let (directory, repository, issue) = fixture();
    let base = revision(directory.path());
    let path = repository.root().join(issue.path);
    let original = fs::read(&path).unwrap();
    fs::write(&path, "---\ninvalid: [\n---\n").unwrap();
    commit(directory.path());
    let head = revision(directory.path());
    fs::write(&path, &original).unwrap();
    let report = ci_validate(directory.path(), &CiValidateRequest { base, head }).unwrap();
    assert!(!report.valid);
    assert!(report.base_report.valid);
    assert!(!report.head_report.errors.is_empty());
    assert_eq!(fs::read(path).unwrap(), original);
}
#[test]
fn missing_commit_does_not_fall_back_to_head() {
    let (directory, _, _) = fixture();
    let result = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: revision(directory.path()),
            head: CiRevision::Commit {
                oid: "0000000000000000000000000000000000000000".parse().unwrap(),
            },
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
}
#[test]
fn full_reference_and_head_resolve_to_exact_identities() {
    let (directory, _, _) = fixture();
    git(directory.path(), &["branch", "ci-base"]);
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: CiRevision::Reference {
                reference: "refs/heads/ci-base".parse().unwrap(),
            },
            head: CiRevision::Head {},
        },
    )
    .unwrap();
    assert!(report.valid);
    assert_eq!(report.base, report.head);
}

#[test]
fn selected_ref_move_is_rejected() {
    let (directory, repository, _) = fixture();
    git(directory.path(), &["branch", "ci-base"]);
    create(&repository, "New head");
    commit(directory.path());
    let result = workdeck_pm::ci_validate_with_faults(
        directory.path(),
        &CiValidateRequest {
            base: CiRevision::Reference {
                reference: "refs/heads/ci-base".parse().unwrap(),
            },
            head: revision(directory.path()),
        },
        |_| {
            git(directory.path(), &["branch", "-f", "ci-base", "HEAD"]);
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
}
#[test]
fn exact_commits_remain_valid_when_unselected_head_moves() {
    let (directory, repository, _) = fixture();
    let base = revision(directory.path());
    create(&repository, "New head");
    commit(directory.path());
    let report = workdeck_pm::ci_validate_with_faults(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
        |_| {
            git(
                directory.path(),
                &["checkout", "--quiet", "--detach", "HEAD~1"],
            );
            Ok(())
        },
    )
    .unwrap();
    assert!(report.valid);
    assert_ne!(
        report.head.commit.as_str(),
        String::from_utf8(git(directory.path(), &["rev-parse", "HEAD"]))
            .unwrap()
            .trim()
    );
}
#[test]
fn cross_repository_candidate_is_rejected() {
    let (directory, repository, _) = fixture();
    let base = revision(directory.path());
    let other = TempDir::new().unwrap();
    let other = Repository::init(other.path(), "OTHER").unwrap();
    fs::copy(
        other.root().join("config.yml"),
        repository.root().join("config.yml"),
    )
    .unwrap();
    commit(directory.path());
    assert_eq!(
        ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path())
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
}
#[test]
fn missing_planning_baseline_is_not_an_empty_valid_contract() {
    let (directory, repository, _) = fixture();
    let head = revision(directory.path());
    git(
        directory.path(),
        &["rm", "-r", "--quiet", "--", ".workdeck"],
    );
    git(
        directory.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "no planning"],
    );
    assert!(
        ci_validate(
            directory.path(),
            &CiValidateRequest {
                base: revision(directory.path()),
                head
            }
        )
        .is_err()
    );
    assert!(!repository.root().join("config.yml").exists());
}
#[test]
fn exact_tag_object_is_not_silently_peeled_to_a_different_commit_identity() {
    let (directory, _, _) = fixture();
    git(
        directory.path(),
        &[
            "-c",
            "tag.gpgSign=false",
            "tag",
            "-a",
            "ci-tag",
            "-m",
            "tag",
        ],
    );
    let oid = String::from_utf8(git(directory.path(), &["rev-parse", "refs/tags/ci-tag"]))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        ci_validate(
            directory.path(),
            &CiValidateRequest {
                base: revision(directory.path()),
                head: CiRevision::Commit { oid }
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
}
#[test]
fn removed_prerequisite_in_candidate_is_not_restored_from_working_files() {
    let (directory, repository, issue) = fixture();
    let prerequisite = create(&repository, "Prerequisite");
    repository
        .mutate_issue_graph(
            issue.metadata.id.as_str(),
            None,
            None,
            &workdeck_pm::IssueGraphMutation::AddPrerequisite {
                prerequisite: prerequisite.metadata.id.to_string(),
            },
            &RequestId::new(),
        )
        .unwrap();
    commit(directory.path());
    let base = revision(directory.path());
    git(
        directory.path(),
        &[
            "rm",
            "--cached",
            "--",
            &format!(".workdeck/{}", prerequisite.path.display()),
        ],
    );
    git(
        directory.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "missing prerequisite",
        ],
    );
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(
        report
            .head_report
            .errors
            .iter()
            .any(|error| error.message.contains(prerequisite.metadata.id.as_str())),
        "{:?}",
        report.head_report.errors
    );
    assert!(repository.root().join(prerequisite.path).is_file());
}

#[test]
fn git_replacement_objects_do_not_change_captured_commit_contents() {
    let (directory, repository, issue) = fixture();
    let base = revision(directory.path());
    let CiRevision::Commit { oid: base_oid } = &base else {
        unreachable!()
    };
    fs::write(repository.root().join(issue.path), "---\ninvalid: [\n---\n").unwrap();
    commit(directory.path());
    let head = revision(directory.path());
    let CiRevision::Commit { oid: head_oid } = &head else {
        unreachable!()
    };
    git(
        directory.path(),
        &["replace", head_oid.as_str(), base_oid.as_str()],
    );
    let report = ci_validate(directory.path(), &CiValidateRequest { base, head }).unwrap();
    assert!(!report.valid);
    assert_eq!(
        serde_json::to_value(report).unwrap()["basis"],
        "planning_source_validation"
    );
}
#[test]
fn git_symlink_record_is_rejected_without_reading_its_target() {
    let (directory, _, issue) = fixture();
    let base = revision(directory.path());
    let oid = String::from_utf8(git(
        directory.path(),
        &[
            "rev-parse",
            &format!("HEAD:.workdeck/{}", issue.path.display()),
        ],
    ))
    .unwrap();
    git(
        directory.path(),
        &[
            "update-index",
            "--cacheinfo",
            &format!("120000,{},.workdeck/{}", oid.trim(), issue.path.display()),
        ],
    );
    git(
        directory.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "unsafe mode"],
    );
    assert_eq!(
        ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path())
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::UnsafePath
    );
}
#[test]
fn revision_deserialization_rejects_expressions_abbreviations_and_options() {
    for name in ["HEAD~1", "--help", "main", "refs/heads/main^{commit}"] {
        assert!(
            serde_json::from_value::<CiRevision>(
                serde_json::json!({"kind":"reference", "reference":name})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<CiRevision>(serde_json::json!({"kind":"commit", "oid":name}))
                .is_err()
        );
    }
    assert!(
        serde_json::from_value::<CiRevision>(serde_json::json!({"kind":"commit", "oid":"123abcd"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<CiRevision>(serde_json::json!({"kind":"head", "trusted":true}))
            .is_err()
    );
}

fn write_definition(repository: &Repository, path: &str, value: &serde_json::Value) {
    let path = repository.root().join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_yaml_ng::to_string(value).unwrap()).unwrap();
}
fn contract_fixture() -> (TempDir, Repository) {
    let (directory, repository, _) = fixture();
    write_definition(
        &repository,
        "commands/unit.yml",
        &serde_json::json!({
            "schema":1, "repository":repository.identity(), "id":"unit", "name":"Unit",
            "recipe":{"kind":"argv", "argv":[{"kind":"literal", "value":"runner"}]},
            "cwd":".", "inputs":{}, "tools":[{"name":"runner", "executable":"/bin/false"}]
        }),
    );
    write_definition(
        &repository,
        "checks/unit.yml",
        &serde_json::json!({
            "schema":1, "repository":repository.identity(), "id":"unit", "name":"Unit",
            "command":"unit", "expectation":{"kind":"process", "allowed_exit_codes":[0]},
            "evaluator_inputs":{}
        }),
    );
    write_definition(
        &repository,
        "check-profiles/required.yml",
        &serde_json::json!({
            "schema":1, "repository":repository.identity(), "id":"required", "name":"Required",
            "checks":["unit"]
        }),
    );
    let mut config = repository.config().unwrap();
    config.acceptance.required_profiles = vec!["required".into()];
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    commit(directory.path());
    let initial = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: revision(directory.path()),
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(initial.valid, "{:?}", initial.head_report.errors);
    (directory, repository)
}
#[test]
fn candidate_cannot_remove_the_required_check_denominator() {
    let (directory, repository) = contract_fixture();
    let base = revision(directory.path());
    let mut config = repository.config().unwrap();
    config.acceptance.required_profiles.clear();
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    for path in [
        "check-profiles/required.yml",
        "checks/unit.yml",
        "commands/unit.yml",
    ] {
        fs::remove_file(repository.root().join(path)).unwrap();
    }
    commit(directory.path());
    assert!(
        !ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path())
            }
        )
        .unwrap()
        .valid
    );
}
#[test]
fn candidate_cannot_weaken_required_expectation_recipe_or_profile() {
    for (path, field, value) in [
        (
            "checks/unit.yml",
            "expectation",
            serde_json::json!({"kind":"process", "allowed_exit_codes":[0,1]}),
        ),
        (
            "commands/unit.yml",
            "tools",
            serde_json::json!([{"name":"runner", "executable":"/bin/true"}]),
        ),
        (
            "check-profiles/required.yml",
            "checks",
            serde_json::json!([]),
        ),
    ] {
        let (directory, repository) = contract_fixture();
        let base = revision(directory.path());
        let mut document: serde_json::Value =
            serde_yaml_ng::from_slice(&fs::read(repository.root().join(path)).unwrap()).unwrap();
        document[field] = value;
        write_definition(&repository, path, &document);
        commit(directory.path());
        assert!(
            !ci_validate(
                directory.path(),
                &CiValidateRequest {
                    base,
                    head: revision(directory.path())
                }
            )
            .unwrap()
            .valid,
            "{path}"
        );
    }
}
#[test]
fn candidate_cannot_weaken_baseline_acceptance_policy() {
    let (directory, repository) = contract_fixture();
    let base = revision(directory.path());
    let mut config = repository.config().unwrap();
    config.acceptance.require_all_criteria = false;
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    commit(directory.path());
    assert!(
        !ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path())
            }
        )
        .unwrap()
        .valid
    );
}

#[test]
fn unchanged_contracts_have_exact_independent_pins_without_executing_recipes() {
    let (directory, repository) = contract_fixture();
    let base = revision(directory.path());
    create(&repository, "Application work");
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid);
    assert!(!report.contracts.review_required);
    assert!(report.contracts.changes.is_empty());
    let base = report.contracts.base.unwrap();
    let head = report.contracts.head.unwrap();
    assert_eq!(base.fingerprint, head.fingerprint);
    assert_eq!(base.checks.len(), 1);
    assert_eq!(base.profiles.len(), 1);
    assert_eq!(base.commands.len(), 1);
    assert_eq!(
        base.commands[0].definition.tools[0].executable,
        "/bin/false"
    );
    assert_eq!(
        base.checks[0].content,
        workdeck_pm::ContentHash::of(&fs::read(repository.root().join("checks/unit.yml")).unwrap())
    );
    assert_ne!(report.base.commit, report.head.commit);
}
#[test]
fn comment_only_contract_edits_change_document_pins_without_semantic_review() {
    let (directory, repository) = contract_fixture();
    let base = revision(directory.path());
    let path = repository.root().join("checks/unit.yml");
    let before = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("# Clarifying comment\n{before}")).unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid);
    assert!(!report.contracts.review_required);
    let base = report.contracts.base.unwrap();
    let head = report.contracts.head.unwrap();
    assert_ne!(base.fingerprint, head.fingerprint);
    assert_ne!(base.checks[0].content, head.checks[0].content);
    assert_eq!(base.checks[0].definition, head.checks[0].definition);
}
#[test]
fn missing_or_archived_required_contract_is_never_an_empty_success() {
    for path in [
        "checks/unit.yml",
        "check-profiles/required.yml",
        "commands/unit.yml",
    ] {
        let (directory, repository) = contract_fixture();
        let base = revision(directory.path());
        let mut document: serde_json::Value =
            serde_yaml_ng::from_slice(&fs::read(repository.root().join(path)).unwrap()).unwrap();
        document["archived"] = serde_json::json!(true);
        write_definition(&repository, path, &document);
        commit(directory.path());
        let report = ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path()),
            },
        )
        .unwrap();
        assert!(!report.valid);
        assert!(report.contracts.base.is_some());
        assert!(report.contracts.head.is_none());
        assert_eq!(
            report.contracts.errors[0].side,
            workdeck_pm::CiContractSide::Head
        );
    }
    let (directory, repository) = contract_fixture();
    let mut config = repository.config().unwrap();
    config.acceptance.required_checks = vec!["absent".into()];
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: revision(directory.path()),
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.contracts.base.is_none());
    assert!(report.contracts.head.is_none());
    assert_eq!(report.contracts.errors.len(), 2);
}
#[test]
fn changed_contract_reports_baseline_and_candidate_definitions_even_when_working_files_are_repaired()
 {
    let (directory, repository) = contract_fixture();
    let base = revision(directory.path());
    let path = repository.root().join("checks/unit.yml");
    let original = fs::read(&path).unwrap();
    let mut document: serde_json::Value = serde_yaml_ng::from_slice(&original).unwrap();
    document["expectation"]["allowed_exit_codes"] = serde_json::json!([0, 1]);
    write_definition(&repository, "checks/unit.yml", &document);
    commit(directory.path());
    fs::write(&path, &original).unwrap();
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.base_report.valid && report.head_report.valid);
    assert!(report.contracts.review_required);
    assert_eq!(report.contracts.changes.len(), 1);
    assert_eq!(
        report.contracts.changes[0].path,
        Path::new("checks/unit.yml")
    );
    assert_ne!(
        report.contracts.base.unwrap().checks[0].definition,
        report.contracts.head.unwrap().checks[0].definition
    );
    assert_eq!(fs::read(path).unwrap(), original);
}

fn acceptance_issue(repository: &Repository) -> IssueRecord {
    let mut input = CreateIssue::new("Accepted behavior", "Body");
    input.fields.insert(
        "acceptance".into(),
        serde_json::json!([
            {"id":"correct", "description":"All records round trip", "checked":false},
            {"id":"bounded", "description":"Reads remain bounded", "checked":false}
        ]),
    );
    serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}
fn write_issue(repository: &Repository, issue: &IssueRecord) {
    fs::write(
        repository.root().join(&issue.path),
        format!(
            "---\n{}---\n{}",
            serde_yaml_ng::to_string(&issue.metadata).unwrap(),
            issue.body
        ),
    )
    .unwrap();
}
#[test]
fn candidate_cannot_delete_or_rewrite_baseline_issue_acceptance() {
    for delete in [false, true] {
        let (directory, repository, _) = fixture();
        let mut issue = acceptance_issue(&repository);
        commit(directory.path());
        let base = revision(directory.path());
        if delete {
            issue.metadata.acceptance.clear();
        } else {
            issue.metadata.acceptance[0].description = "Some records round trip".into();
        }
        write_issue(&repository, &issue);
        commit(directory.path());
        let report = ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path()),
            },
        )
        .unwrap();
        assert!(
            !report.valid,
            "candidate silently changed baseline acceptance (delete={delete})"
        );
    }
}

#[test]
fn checkbox_prose_and_criterion_order_changes_preserve_semantic_requirements() {
    let (directory, repository, _) = fixture();
    let mut issue = acceptance_issue(&repository);
    commit(directory.path());
    let base = revision(directory.path());
    issue.metadata.acceptance[0].checked = true;
    issue.metadata.acceptance.reverse();
    issue.body.push_str("\nProgress note\n");
    write_issue(&repository, &issue);
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid, "{:?}", report.head_report.errors);
    assert!(!report.contracts.review_required);
    let base = report.contracts.base.unwrap();
    let head = report.contracts.head.unwrap();
    assert_eq!(base.subjects.len(), 1);
    assert_eq!(base.subjects[0].requirements, head.subjects[0].requirements);
    assert_ne!(base.subjects[0].content, head.subjects[0].content);
    assert_ne!(base.fingerprint, head.fingerprint);
}
#[test]
fn deleting_an_entire_accepted_issue_requires_review() {
    let (directory, repository, _) = fixture();
    let issue = acceptance_issue(&repository);
    commit(directory.path());
    let base = revision(directory.path());
    fs::remove_file(repository.root().join(&issue.path)).unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.contracts.review_required);
    assert_eq!(
        report.contracts.changes[0].subject,
        Some(workdeck_pm::CiSubjectIdentity::Issue {
            id: issue.metadata.id
        })
    );
    assert!(report.contracts.changes[0].head.is_none());
}
#[test]
fn project_exit_and_milestone_outcome_definitions_are_bound_to_the_baseline() {
    for (kind, field) in [
        (workdeck_pm::PlanningKind::Project, "exit_criteria"),
        (workdeck_pm::PlanningKind::Milestone, "outcomes"),
    ] {
        let (directory, repository, _) = fixture();
        let mut input = workdeck_pm::CreatePlanning::new("Delivery");
        if kind == workdeck_pm::PlanningKind::Milestone {
            let owner: workdeck_pm::PlanningRecord = serde_json::from_value(
                repository
                    .create_planning(
                        workdeck_pm::PlanningKind::Project,
                        &workdeck_pm::CreatePlanning::new("Owner"),
                        &RequestId::new(),
                    )
                    .unwrap()
                    .result,
            )
            .unwrap();
            input
                .fields
                .insert("project".into(), serde_json::json!(owner.metadata.id));
        }
        input.fields.insert(
            field.into(),
            serde_json::json!([{"id":"ready","description":"Every required behavior passes"}]),
        );
        let mut record: workdeck_pm::PlanningRecord = serde_json::from_value(
            repository
                .create_planning(kind, &input, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        commit(directory.path());
        let base = revision(directory.path());
        if kind == workdeck_pm::PlanningKind::Project {
            record.metadata.exit_criteria.clear();
        } else {
            record.metadata.outcomes[0].description = "Some behaviors pass".into();
        }
        fs::write(
            repository.root().join(&record.path),
            format!(
                "---\n{}---\n{}",
                serde_yaml_ng::to_string(&record.metadata).unwrap(),
                record.body
            ),
        )
        .unwrap();
        commit(directory.path());
        let report = ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path()),
            },
        )
        .unwrap();
        assert!(!report.valid);
        assert!(report.contracts.review_required);
        assert_eq!(
            report.contracts.changes[0].subject,
            Some(workdeck_pm::CiSubjectIdentity::Planning {
                record_kind: kind,
                id: record.metadata.id
            })
        );
    }
}
#[test]
fn physical_feature_moves_preserve_contract_identity_but_criteria_changes_require_review() {
    let (directory, repository, _) = fixture();
    let mut input = workdeck_pm::CreateFeature::new("Capability");
    input.fields.insert(
        "criteria".into(),
        serde_json::json!([{"id":"safe","description":"Writes preserve all unrelated data"}]),
    );
    let feature: workdeck_pm::FeatureRecord = serde_json::from_value(
        repository
            .create_feature(&input, &RequestId::new())
            .unwrap()
            .result["record"]
            .clone(),
    )
    .unwrap();
    commit(directory.path());
    let base = revision(directory.path());
    repository
        .mutate_feature(
            feature.metadata.id.as_str(),
            Some(&feature.source),
            &workdeck_pm::FeatureMutation::Relocate {
                directory: "delivery/native".into(),
            },
            &RequestId::new(),
        )
        .unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: base.clone(),
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid, "{:?}", report.head_report.errors);
    let before = report.contracts.base.unwrap();
    let after = report.contracts.head.unwrap();
    assert_eq!(before.subjects[0].subject, after.subjects[0].subject);
    assert_ne!(before.subjects[0].path, after.subjects[0].path);
    let moved = repository.feature(feature.metadata.id.as_str()).unwrap();
    repository
        .mutate_feature(
            feature.metadata.id.as_str(),
            Some(&moved.source),
            &workdeck_pm::FeatureMutation::Update {
                fields: std::collections::BTreeMap::from([(
                    "criteria".into(),
                    serde_json::json!([]),
                )]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.contracts.review_required);
    assert_eq!(
        report.contracts.changes[0].subject,
        Some(workdeck_pm::CiSubjectIdentity::Feature {
            id: feature.metadata.id
        })
    );
}
#[test]
fn gate_freshness_and_producer_check_pins_cannot_be_weakened_by_the_candidate() {
    let (directory, repository, _) = fixture();
    let issue = acceptance_issue(&repository);
    let criterion = repository
        .resolve_criterion(
            &workdeck_pm::CriterionOwner::Issue(issue.metadata.id.clone()),
            "correct",
        )
        .unwrap()
        .reference;
    let input = workdeck_pm::CreateGate {
        name: "Required gate".into(),
        description: String::new(),
        requirements: vec![workdeck_pm::GateRequirement {
            id: "behavior".into(),
            criterion,
            producer: workdeck_pm::ProducerRef {
                id: "runner".into(),
                definition: workdeck_pm::ContentHash::of(b"runner v1"),
            },
            check: workdeck_pm::CheckRef {
                id: "roundtrip".into(),
                definition: workdeck_pm::ContentHash::of(b"check v1"),
            },
            max_age_seconds: Some(60),
        }],
        custom: Default::default(),
        extra: Default::default(),
    };
    let mut gate: workdeck_pm::GateRecord =
        serde_json::from_value::<workdeck_pm::GateMutationResult>(
            repository
                .create_gate(&input, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap()
        .gate;
    commit(directory.path());
    let base = revision(directory.path());
    gate.definition.requirements[0].max_age_seconds = None;
    gate.definition.requirements[0].check.definition =
        workdeck_pm::ContentHash::of(b"easier check");
    fs::write(
        repository.root().join(&gate.path),
        serde_yaml_ng::to_string(&gate.definition).unwrap(),
    )
    .unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.contracts.review_required);
    assert_eq!(report.contracts.changes.len(), 1);
    assert_eq!(
        report.contracts.changes[0].subject,
        Some(workdeck_pm::CiSubjectIdentity::Gate {
            id: gate.definition.id
        })
    );
}

#[test]
fn removing_a_baseline_prerequisite_requires_review_even_without_acceptance_checkboxes() {
    let (directory, repository, issue) = fixture();
    let prerequisite = create(&repository, "Prerequisite");
    repository
        .mutate_issue_graph(
            issue.metadata.id.as_str(),
            None,
            None,
            &workdeck_pm::IssueGraphMutation::AddPrerequisite {
                prerequisite: prerequisite.metadata.id.to_string(),
            },
            &RequestId::new(),
        )
        .unwrap();
    commit(directory.path());
    let base = revision(directory.path());
    let mut issue = repository
        .list_issues()
        .unwrap()
        .into_iter()
        .find(|record| record.metadata.id == issue.metadata.id)
        .unwrap();
    issue.metadata.prerequisites.clear();
    write_issue(&repository, &issue);
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.base_report.valid && report.head_report.valid);
    assert!(report.contracts.review_required);
    assert_eq!(
        report.contracts.changes[0].subject,
        Some(workdeck_pm::CiSubjectIdentity::Issue {
            id: issue.metadata.id
        })
    );
}

#[test]
fn candidate_cannot_reclassify_completion_states_without_contract_review() {
    let (directory, repository, _) = fixture();
    let base = revision(directory.path());
    let mut config = repository.config().unwrap();
    config
        .workflow
        .states
        .iter_mut()
        .find(|state| state.id == "done")
        .unwrap()
        .category = workdeck_pm::WorkflowCategory::Canceled;
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.base_report.valid && report.head_report.valid);
    assert!(
        !report.valid,
        "candidate changed completion semantics without review"
    );
}

#[test]
fn candidate_cannot_remove_inherited_feature_requirements() {
    let (directory, repository, _) = fixture();
    let mut input = workdeck_pm::CreateFeature::new("Inherited contract");
    input.fields.insert(
        "criteria".into(),
        serde_json::json!([{"id":"safe","description":"Preserve every record"}]),
    );
    let feature: workdeck_pm::FeatureRecord = serde_json::from_value(
        repository
            .create_feature(&input, &RequestId::new())
            .unwrap()
            .result["record"]
            .clone(),
    )
    .unwrap();
    let mut input = CreateIssue::new("Implementation", "");
    input
        .fields
        .insert("features".into(), serde_json::json!([feature.metadata.id]));
    let mut issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    commit(directory.path());
    let base = revision(directory.path());
    issue.metadata.features.clear();
    write_issue(&repository, &issue);
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.base_report.valid && report.head_report.valid);
    assert!(
        !report.valid,
        "candidate dropped inherited feature criteria without review"
    );
}

#[test]
fn feature_decision_changes_require_review_even_before_criteria_are_declared() {
    let (directory, repository, _) = fixture();
    let feature: workdeck_pm::FeatureRecord = serde_json::from_value(
        repository
            .create_feature(
                &workdeck_pm::CreateFeature::new("Proposed capability"),
                &RequestId::new(),
            )
            .unwrap()
            .result["record"]
            .clone(),
    )
    .unwrap();
    commit(directory.path());
    let base = revision(directory.path());
    repository
        .mutate_feature(
            feature.metadata.id.as_str(),
            Some(&feature.source),
            &workdeck_pm::FeatureMutation::Update {
                fields: std::collections::BTreeMap::from([(
                    "decision".into(),
                    serde_json::json!("deferred"),
                )]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.contracts.review_required);
    assert_eq!(
        report.contracts.changes[0].subject,
        Some(workdeck_pm::CiSubjectIdentity::Feature {
            id: feature.metadata.id
        })
    );
}

fn evaluator_fixture() -> (TempDir, Repository) {
    let (directory, repository) = contract_fixture();
    fs::create_dir(directory.path().join("tests")).unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(
        directory.path().join("tests/evaluator.py"),
        "assert value == 42\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("src/app.txt"),
        "candidate application\n",
    )
    .unwrap();
    let path = repository.root().join("commands/unit.yml");
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    command["inputs"] = serde_json::json!({"files":["src/app.txt"],"trees":["tests"]});
    write_definition(&repository, "commands/unit.yml", &command);
    let mut check: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("checks/unit.yml")).unwrap())
            .unwrap();
    check["evaluator_inputs"] = serde_json::json!({"trees":["tests"]});
    write_definition(&repository, "checks/unit.yml", &check);
    git(directory.path(), &["add", "--", "tests", "src"]);
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: revision(directory.path()),
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid, "{:?}", report.contracts.errors);
    (directory, repository)
}
#[test]
fn candidate_cannot_weaken_evaluator_code_while_retaining_check_metadata() {
    let (directory, _) = evaluator_fixture();
    let base = revision(directory.path());
    let path = directory.path().join("tests/evaluator.py");
    let before = fs::read(&path).unwrap();
    fs::write(&path, "assert True\n").unwrap();
    git(directory.path(), &["add", "--", "tests/evaluator.py"]);
    git(
        directory.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "weaker evaluator",
        ],
    );
    fs::write(&path, &before).unwrap();
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(
        !report.valid,
        "evaluator code changed without contract review"
    );
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn application_changes_do_not_change_the_evaluator_contract() {
    let (directory, _) = evaluator_fixture();
    let base = revision(directory.path());
    fs::write(directory.path().join("src/app.txt"), "fixed application\n").unwrap();
    git(directory.path(), &["add", "--", "src/app.txt"]);
    git(
        directory.path(),
        &[
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "application fix",
        ],
    );
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid, "{:?}", report.contracts.errors);
    assert!(report.contracts.evaluator_changes.is_empty());
    let before = report.contracts.base.unwrap();
    let after = report.contracts.head.unwrap();
    assert_eq!(before.evaluators, after.evaluators);
    assert_ne!(report.base.tree, report.head.tree);
    assert!(before.evaluators[0].entries.iter().any(|entry| matches!(entry, workdeck_pm::CiEvaluatorEntry::File {path, content, ..} if path == Path::new("tests/evaluator.py") && content == &workdeck_pm::ContentHash::of(b"assert value == 42\n"))));
}
#[test]
fn evaluator_tree_membership_and_modes_are_reviewed_without_reading_dirty_files() {
    for mode_change in [false, true] {
        let (directory, _) = evaluator_fixture();
        let base = revision(directory.path());
        if mode_change {
            git(
                directory.path(),
                &["update-index", "--chmod=+x", "--", "tests/evaluator.py"],
            );
        } else {
            fs::write(
                directory.path().join("tests/extra.py"),
                "new proposed test\n",
            )
            .unwrap();
            git(directory.path(), &["add", "--", "tests/extra.py"]);
        }
        git(
            directory.path(),
            &[
                "commit",
                "--quiet",
                "--no-gpg-sign",
                "-m",
                "evaluation changes",
            ],
        );
        fs::write(
            directory.path().join("tests/evaluator.py"),
            "dirty local evaluator\n",
        )
        .unwrap();
        let report = ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path()),
            },
        )
        .unwrap();
        assert!(!report.valid);
        assert!(report.contracts.review_required);
        assert_eq!(report.contracts.evaluator_changes.len(), 1);
        assert_eq!(report.contracts.evaluator_changes[0].check, "unit");
        assert!(report.contracts.changes.is_empty());
    }
}
#[test]
fn omitted_ci_evaluator_selection_is_not_an_empty_evaluator_and_local_catalog_still_loads() {
    let (directory, repository) = contract_fixture();
    let mut check: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("checks/unit.yml")).unwrap())
            .unwrap();
    check.as_object_mut().unwrap().remove("evaluator_inputs");
    write_definition(&repository, "checks/unit.yml", &check);
    assert!(
        repository
            .check("unit")
            .unwrap()
            .definition
            .evaluator_inputs
            .is_none()
    );
    let serialized = serde_json::to_value(repository.check("unit").unwrap().definition).unwrap();
    assert!(serialized.get("evaluator_inputs").is_none());
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: revision(directory.path()),
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!report.valid);
    assert_eq!(report.contracts.errors.len(), 2);
    assert!(report.contracts.errors.iter().all(|diagnostic| {
        diagnostic
            .error
            .message
            .contains("explicit evaluator_inputs")
    }));
}
#[test]
fn missing_or_symlinked_evaluator_inputs_are_rejected() {
    for symlink in [false, true] {
        let (directory, repository) = evaluator_fixture();
        let base = revision(directory.path());
        if symlink {
            let oid = String::from_utf8(git(
                directory.path(),
                &["rev-parse", "HEAD:tests/evaluator.py"],
            ))
            .unwrap();
            git(
                directory.path(),
                &[
                    "update-index",
                    "--cacheinfo",
                    &format!("120000,{},tests/evaluator.py", oid.trim()),
                ],
            );
        } else {
            let mut check: serde_json::Value = serde_yaml_ng::from_slice(
                &fs::read(repository.root().join("checks/unit.yml")).unwrap(),
            )
            .unwrap();
            check["evaluator_inputs"] = serde_json::json!({"files":["tests/missing.py"]});
            write_definition(&repository, "checks/unit.yml", &check);
            git(directory.path(), &["add", "--", ".workdeck"]);
        }
        git(
            directory.path(),
            &[
                "commit",
                "--quiet",
                "--no-gpg-sign",
                "-m",
                "invalid evaluator input",
            ],
        );
        let report = ci_validate(
            directory.path(),
            &CiValidateRequest {
                base,
                head: revision(directory.path()),
            },
        )
        .unwrap();
        assert!(!report.valid);
        assert!(report.contracts.head.is_none());
        assert_eq!(
            report.contracts.errors[0].side,
            workdeck_pm::CiContractSide::Head
        );
        assert_eq!(
            report.contracts.errors[0].error.code,
            if symlink {
                ErrorCode::UnsafePath
            } else {
                ErrorCode::PolicyBlocked
            }
        );
    }
}
#[test]
fn evaluator_selection_cannot_omit_runtime_dependency_tracking_or_select_engine_output() {
    let (_directory, repository) = contract_fixture();
    let mut check: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("checks/unit.yml")).unwrap())
            .unwrap();
    for path in [
        "untracked-test.py",
        ".git/config",
        ".workdeck/runs/result.yml",
        "../outside",
        "tests/*.py",
    ] {
        check["evaluator_inputs"] = serde_json::json!({"files":[path]});
        write_definition(&repository, "checks/unit.yml", &check);
        assert!(repository.check("unit").is_err(), "{path}");
    }
}

#[test]
fn evaluator_file_selectors_reject_case_aliased_parent_directories() {
    let (directory, repository) = evaluator_fixture();
    let oid = String::from_utf8(git(
        directory.path(),
        &["rev-parse", "HEAD:tests/evaluator.py"],
    ))
    .unwrap();
    git(
        directory.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{},Tests/extra.py", oid.trim()),
        ],
    );
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("commands/unit.yml")).unwrap())
            .unwrap();
    command["inputs"] = serde_json::json!({"files":["tests/evaluator.py","Tests/extra.py"]});
    write_definition(&repository, "commands/unit.yml", &command);
    let mut check: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("checks/unit.yml")).unwrap())
            .unwrap();
    check["evaluator_inputs"] =
        serde_json::json!({"files":["tests/evaluator.py","Tests/extra.py"]});
    write_definition(&repository, "checks/unit.yml", &check);
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: revision(directory.path()),
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(
        !report.valid,
        "case-aliased evaluator directories were accepted"
    );
    assert!(
        report
            .contracts
            .errors
            .iter()
            .any(|error| error.error.code == ErrorCode::UnsafePath)
    );
}
#[test]
fn evaluator_root_selection_excludes_engine_output_but_captures_repository_files() {
    let (directory, repository) = evaluator_fixture();
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("commands/unit.yml")).unwrap())
            .unwrap();
    command["inputs"] = serde_json::json!({"trees":["."]});
    write_definition(&repository, "commands/unit.yml", &command);
    let mut check: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("checks/unit.yml")).unwrap())
            .unwrap();
    check["evaluator_inputs"] = serde_json::json!({"trees":["."]});
    write_definition(&repository, "checks/unit.yml", &check);
    commit(directory.path());
    let base = revision(directory.path());
    fs::create_dir_all(repository.root().join(".local")).unwrap();
    fs::write(
        repository.root().join(".local/noise.txt"),
        "irrelevant engine output",
    )
    .unwrap();
    git(
        directory.path(),
        &["add", "--force", "--", ".workdeck/.local/noise.txt"],
    );
    git(
        directory.path(),
        &["commit", "--quiet", "--no-gpg-sign", "-m", "engine output"],
    );
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid, "{:?}", report.contracts.errors);
    let manifests = report.contracts.head.unwrap().evaluators;
    assert!(
        manifests[0]
            .entries
            .iter()
            .any(|entry| entry.path() == Path::new("tests/evaluator.py"))
    );
    assert!(
        !manifests[0]
            .entries
            .iter()
            .any(|entry| entry.path().starts_with(".workdeck/.local")
                || entry.path().starts_with(".workdeck/operations"))
    );
    assert!(report.contracts.evaluator_changes.is_empty());
}

fn binding_fixture() -> (TempDir, Repository) {
    let (directory, repository) = evaluator_fixture();
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("commands/unit.yml")).unwrap())
            .unwrap();
    command["tools"][0]["executable"] = serde_json::json!("/bin/sh");
    write_definition(&repository, "commands/unit.yml", &command);
    commit(directory.path());
    (directory, repository)
}

#[test]
fn ci_check_binding_rejects_dirty_candidate_inputs_even_when_local_plan_is_current() {
    let (directory, repository) = binding_fixture();
    let source = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: revision(directory.path()),
            head: revision(directory.path()),
        },
    )
    .unwrap()
    .head;
    fs::write(
        directory.path().join("src/app.txt"),
        "uncommitted candidate",
    )
    .unwrap();
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let error = workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan).unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(error.path.as_deref(), Some("src/app.txt"));
    assert_eq!(error.details.unwrap()["invocation"], "unit");
}

fn binding_source(root: &Path) -> workdeck_pm::CiSourceIdentity {
    ci_validate(
        root,
        &CiValidateRequest {
            base: revision(root),
            head: revision(root),
        },
    )
    .unwrap()
    .head
}

#[test]
fn ci_check_binding_pins_matching_inputs_without_granting_ci_trust() {
    let (directory, repository) = binding_fixture();
    let source = binding_source(directory.path());
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    let binding = workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan).unwrap();
    assert_eq!(binding.source, source);
    assert_eq!(binding.plan, plan.fingerprint);
    assert_eq!(plan.basis, workdeck_pm::VerificationBasis::LocalFeedback);
    assert_eq!(binding.inputs.len(), 1);
    assert!(
        binding.inputs[0]
            .entries
            .iter()
            .any(|entry| entry.path() == Path::new("src/app.txt"))
    );
    let again = workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan).unwrap();
    assert_eq!(binding, again);
    assert!(!repository.root().join(".local/runs").exists());
}

#[test]
fn ci_check_binding_rejects_untracked_selected_members_and_changed_modes() {
    use std::os::unix::fs::PermissionsExt;
    for mode in [false, true] {
        let (directory, repository) = binding_fixture();
        let source = binding_source(directory.path());
        if mode {
            fs::set_permissions(
                directory.path().join("tests/evaluator.py"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        } else {
            fs::write(directory.path().join("tests/untracked.py"), "assert True\n").unwrap();
        }
        let plan = repository
            .check_plan(&workdeck_pm::CheckPlanRequest::default())
            .unwrap();
        assert_eq!(
            workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan)
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
    }
}

#[test]
fn ci_check_binding_verifies_optional_absence_against_the_commit() {
    for committed in [false, true] {
        let (directory, repository) = binding_fixture();
        let mut command: serde_json::Value = serde_yaml_ng::from_slice(
            &fs::read(repository.root().join("commands/unit.yml")).unwrap(),
        )
        .unwrap();
        command["inputs"]["optional_files"] = serde_json::json!(["optional.txt"]);
        write_definition(&repository, "commands/unit.yml", &command);
        if committed {
            fs::write(directory.path().join("optional.txt"), "required if present").unwrap();
            git(directory.path(), &["add", "--", "optional.txt"]);
        }
        commit(directory.path());
        let source = binding_source(directory.path());
        if committed {
            fs::remove_file(directory.path().join("optional.txt")).unwrap();
        }
        let plan = repository
            .check_plan(&workdeck_pm::CheckPlanRequest::default())
            .unwrap();
        let result = workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan);
        if committed {
            assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
        } else {
            result.unwrap();
        }
    }
}

#[test]
fn ci_check_binding_rejects_dirty_definitions_and_forged_source_pins() {
    let (directory, repository) = binding_fixture();
    let source = binding_source(directory.path());
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    let mut forged = source.clone();
    forged.content = workdeck_pm::ContentHash::of(b"forged");
    assert_eq!(
        workdeck_pm::bind_ci_check_plan(directory.path(), &forged, &plan)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("commands/unit.yml")).unwrap())
            .unwrap();
    command["name"] = serde_json::json!("Changed command");
    write_definition(&repository, "commands/unit.yml", &command);
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    assert_eq!(
        workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn ci_check_binding_captures_repository_tools_outside_file_selectors() {
    use std::os::unix::fs::PermissionsExt;
    let (directory, repository) = binding_fixture();
    fs::create_dir(directory.path().join("bin")).unwrap();
    let tool = directory.path().join("bin/runner.sh");
    fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("commands/unit.yml")).unwrap())
            .unwrap();
    command["tools"][0]["executable"] = serde_json::json!("bin/runner.sh");
    write_definition(&repository, "commands/unit.yml", &command);
    git(directory.path(), &["add", "--", "bin"]);
    commit(directory.path());
    let source = binding_source(directory.path());
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let binding = workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan).unwrap();
    assert!(
        binding.inputs[0]
            .entries
            .iter()
            .any(|entry| entry.path() == Path::new("bin/runner.sh"))
    );
    fs::write(&tool, "#!/bin/sh\nexit 1\n").unwrap();
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    assert_eq!(
        workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn ci_check_binding_root_selection_matches_committed_membership() {
    let (directory, repository) = binding_fixture();
    let mut command: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(repository.root().join("commands/unit.yml")).unwrap())
            .unwrap();
    command["inputs"] = serde_json::json!({"trees":["."]});
    write_definition(&repository, "commands/unit.yml", &command);
    commit(directory.path());
    // Initialization leaves empty planning directories that Git cannot represent.
    // Exercise the clean CI checkout, not those untracked fixture directories.
    let checkout = tempfile::tempdir().unwrap();
    git(
        directory.path(),
        &[
            "clone",
            "--quiet",
            "--no-hardlinks",
            directory.path().to_str().unwrap(),
            checkout.path().to_str().unwrap(),
        ],
    );
    let directory = checkout;
    let repository = Repository::open_source(&directory.path().join(".workdeck")).unwrap();
    let source = binding_source(directory.path());
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan).unwrap();
    fs::create_dir(directory.path().join("empty-untracked")).unwrap();
    let plan = repository
        .check_plan(&workdeck_pm::CheckPlanRequest::default())
        .unwrap();
    assert_eq!(
        workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn ci_rejects_candidate_organization_policy_violations_but_allows_repairs() {
    let (directory, repository, mut issue) = fixture();
    let base = revision(directory.path());
    let mut schema =
        serde_json::to_value(repository.organization_schema().unwrap().definition).unwrap();
    schema["fields"] = serde_json::json!({"risk": {
        "type":"text", "scopes":["issue"], "required":true
    }});
    write_definition(&repository, "schema.yml", &schema);
    commit(directory.path());
    let broken = revision(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: broken.clone(),
        },
    )
    .unwrap();
    assert!(
        report.head_report.valid,
        "policy violations remain structurally readable"
    );
    assert!(!report.head_report.policy_compliant);
    assert!(
        !report.valid,
        "CI must reject structurally valid policy violations"
    );
    issue
        .metadata
        .custom
        .insert("risk".into(), serde_json::json!("low"));
    write_issue(&repository, &issue);
    commit(directory.path());
    let repaired = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: broken,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(!repaired.base_report.policy_compliant);
    assert!(repaired.head_report.policy_compliant);
    assert!(
        repaired.valid,
        "repairing data under unchanged policy must remain possible"
    );
}

#[test]
fn ci_requires_review_for_organization_policy_removal() {
    let (directory, repository, _) = fixture();
    let mut schema =
        serde_json::to_value(repository.organization_schema().unwrap().definition).unwrap();
    schema["unit_mode"] = serde_json::json!("registered");
    write_definition(&repository, "schema.yml", &schema);
    commit(directory.path());
    let base = revision(directory.path());
    fs::remove_file(repository.root().join("schema.yml")).unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.head_report.policy_compliant);
    assert!(
        report.contracts.review_required,
        "removing registered unit authority requires review"
    );
    assert!(
        report
            .contracts
            .changes
            .iter()
            .any(|change| change.path == Path::new("schema.yml"))
    );
    assert!(!report.valid);
}

#[test]
fn ci_identity_authority_changes_require_review_but_display_edits_do_not() {
    let (directory, repository, _) = fixture();
    let mut users = serde_json::to_value(repository.users().unwrap().registry).unwrap();
    users["users"] = serde_json::json!({"alice":{"name":"Alice", "kind":"human"}});
    write_definition(&repository, "users.yml", &users);
    commit(directory.path());
    let base = revision(directory.path());
    users["users"]["alice"]["name"] = serde_json::json!("Alice Smith");
    write_definition(&repository, "users.yml", &users);
    commit(directory.path());
    let cosmetic = revision(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: base.clone(),
            head: cosmetic.clone(),
        },
    )
    .unwrap();
    assert!(report.valid);
    assert_ne!(
        report.contracts.base.unwrap().fingerprint,
        report.contracts.head.unwrap().fingerprint
    );
    users["mode"] = serde_json::json!("registered");
    write_definition(&repository, "users.yml", &users);
    commit(directory.path());
    let restricted = revision(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: cosmetic,
            head: restricted.clone(),
        },
    )
    .unwrap();
    assert!(report.contracts.review_required);
    fs::remove_file(repository.root().join("users.yml")).unwrap();
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: restricted,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.contracts.review_required);
    assert!(
        report
            .contracts
            .changes
            .iter()
            .any(|change| change.path == Path::new("users.yml"))
    );
}

#[test]
fn ci_binding_and_preparation_reject_noncompliant_committed_policy() {
    let (directory, repository) = binding_fixture();
    let mut schema =
        serde_json::to_value(repository.organization_schema().unwrap().definition).unwrap();
    schema["fields"] = serde_json::json!({"risk": {
        "type":"text", "scopes":["issue"], "required":true
    }});
    write_definition(&repository, "schema.yml", &schema);
    commit(directory.path());
    let source = binding_source(directory.path());
    let request = workdeck_pm::CheckPlanRequest::default();
    let plan = repository.check_plan(&request).unwrap();
    let error = workdeck_pm::bind_ci_check_plan(directory.path(), &source, &plan).unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyBlocked);
    assert_eq!(
        error.details.unwrap()["validation"]["policy_compliant"],
        false
    );
    let error =
        workdeck_pm::prepare_ci_check(directory.path(), &revision(directory.path()), &request)
            .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyBlocked);
}

#[test]
fn ci_materializing_default_organization_files_does_not_change_authority() {
    let (directory, repository, _) = fixture();
    let base = revision(directory.path());
    write_definition(
        &repository,
        "users.yml",
        &serde_json::to_value(repository.users().unwrap().registry).unwrap(),
    );
    write_definition(
        &repository,
        "schema.yml",
        &serde_json::to_value(repository.organization_schema().unwrap().definition).unwrap(),
    );
    commit(directory.path());
    let report = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
    )
    .unwrap();
    assert!(report.valid);
    let before = report.contracts.base.unwrap();
    let after = report.contracts.head.unwrap();
    assert!(before.organization.users.source.is_none());
    assert!(after.organization.users.source.is_some());
    assert_ne!(before.fingerprint, after.fingerprint);
}

#[test]
fn pinned_baseline_rejects_substitution_even_when_candidate_is_valid() {
    let (directory, repository, _) = fixture();
    let base = revision(directory.path());
    let original = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: base.clone(),
            head: base.clone(),
        },
    )
    .unwrap();
    let pin = workdeck_pm::CiBaselinePin {
        commit: original.base.commit.clone(),
        contract: original
            .contracts
            .base
            .as_ref()
            .unwrap()
            .fingerprint
            .clone(),
    };
    create(&repository, "Another candidate");
    commit(directory.path());
    let head = revision(directory.path());
    let report = workdeck_pm::ci_validate_pinned(
        directory.path(),
        &CiValidateRequest {
            base: base.clone(),
            head: head.clone(),
        },
        &pin,
    )
    .unwrap();
    assert!(report.valid);
    assert_eq!(
        report.basis,
        workdeck_pm::CiValidationBasis::PinnedBaselineValidation
    );
    assert_eq!(report.baseline_pin, Some(pin.clone()));
    let error = workdeck_pm::ci_validate_pinned(
        directory.path(),
        &CiValidateRequest {
            base: head.clone(),
            head: head.clone(),
        },
        &pin,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyBlocked);
    let wrong_contract = workdeck_pm::CiBaselinePin {
        contract: workdeck_pm::ContentHash::of(b"unaccepted contract"),
        ..pin
    };
    assert_eq!(
        workdeck_pm::ci_validate_pinned(
            directory.path(),
            &CiValidateRequest { base, head },
            &wrong_contract
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
}

#[test]
fn pinned_baseline_does_not_approve_weakened_candidate_contract() {
    let (directory, repository) = contract_fixture();
    let base = revision(directory.path());
    let accepted = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: base.clone(),
            head: base.clone(),
        },
    )
    .unwrap();
    let pin = workdeck_pm::CiBaselinePin {
        commit: accepted.base.commit,
        contract: accepted.contracts.base.unwrap().fingerprint,
    };
    let original = fs::read(repository.root().join("checks/unit.yml")).unwrap();
    let mut document: serde_json::Value = serde_yaml_ng::from_slice(&original).unwrap();
    document["expectation"]["allowed_exit_codes"] = serde_json::json!([0, 1]);
    write_definition(&repository, "checks/unit.yml", &document);
    commit(directory.path());
    let report = workdeck_pm::ci_validate_pinned(
        directory.path(),
        &CiValidateRequest {
            base,
            head: revision(directory.path()),
        },
        &pin,
    )
    .unwrap();
    assert!(!report.valid);
    assert!(report.contracts.review_required);
}

#[test]
fn signed_contract_review_admits_exact_change_and_rejects_replayed_candidate() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use ed25519_dalek::{Signer, SigningKey};
    use workdeck_pm::*;
    let (directory, repository) = contract_fixture();
    fs::create_dir_all(directory.path().join("evaluators")).unwrap();
    fs::write(directory.path().join("evaluators/unit.sh"), b"exit 0\n").unwrap();
    git(directory.path(), &["add", "--", "evaluators"]);
    for (path, field) in [
        ("commands/unit.yml", "inputs"),
        ("checks/unit.yml", "evaluator_inputs"),
    ] {
        let mut value: serde_json::Value =
            serde_yaml_ng::from_slice(&fs::read(repository.root().join(path)).unwrap()).unwrap();
        value[field] = serde_json::json!({"trees":["evaluators"]});
        write_definition(&repository, path, &value);
    }
    let mut contextual_issue = create(&repository, "Reviewed task");
    contextual_issue.metadata.acceptance.push(
        serde_json::from_value(serde_json::json!({
            "id":"reviewed", "description":"Reviewed requirement", "checked":false
        }))
        .unwrap(),
    );
    write_issue(&repository, &contextual_issue);
    commit(directory.path());

    let base = revision(directory.path());
    let accepted = ci_validate(
        directory.path(),
        &CiValidateRequest {
            base: base.clone(),
            head: base.clone(),
        },
    )
    .unwrap();
    let baseline = CiBaselinePin {
        commit: accepted.base.commit,
        contract: accepted.contracts.base.unwrap().fingerprint,
    };
    let original = fs::read(repository.root().join("checks/unit.yml")).unwrap();
    let mut definition: serde_json::Value = serde_yaml_ng::from_slice(&original).unwrap();
    definition["expectation"]["allowed_exit_codes"] = serde_json::json!([0, 1]);
    write_definition(&repository, "checks/unit.yml", &definition);
    commit(directory.path());
    let request = CiValidateRequest {
        base,
        head: revision(directory.path()),
    };
    let candidate = ci_validate_pinned(directory.path(), &request, &baseline).unwrap();
    assert!(!candidate.valid && candidate.contracts.review_required);
    let now = chrono::Utc::now();
    let signing = SigningKey::from_bytes(&[43; 32]);
    let policy = ContractReviewPolicy {
        schema: SchemaVersion::CURRENT,
        repository: repository.identity().clone(),
        required_reviewers: vec![ContractReviewer {
            id: "maintainer".into(),
            public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
            not_before: now - chrono::Duration::hours(1),
            expires_at: now + chrono::Duration::hours(1),
        }],
    };
    let approval = CiContractApproval {
        schema: SchemaVersion::CURRENT,
        repository: repository.identity().clone(),
        baseline: baseline.clone(),
        head: candidate.head,
        head_contract: candidate.contracts.head.unwrap().fingerprint,
        decision: ContractReviewDecision::Approve,
        reviewed_at: now,
        expires_at: now + chrono::Duration::minutes(30),
    };
    let payload = serde_json::to_vec(&approval).unwrap();
    let mut message = format!(
        "DSSEv1 {} {} {} ",
        CONTRACT_REVIEW_PAYLOAD_TYPE.len(),
        CONTRACT_REVIEW_PAYLOAD_TYPE,
        payload.len()
    )
    .into_bytes();
    message.extend_from_slice(&payload);
    let envelope = SignedContractReview {
        payload_type: CONTRACT_REVIEW_PAYLOAD_TYPE.into(),
        payload: STANDARD.encode(payload),
        signatures: vec![ReportSignature {
            keyid: Some("untrusted-hint".into()),
            sig: STANDARD.encode(signing.sign(&message).to_bytes()),
        }],
    };
    let pin = policy.fingerprint().unwrap();
    let mut forged = envelope.clone();
    forged.payload_type = CHECK_REPORT_PAYLOAD_TYPE.into();
    assert!(
        ci_validate_reviewed(
            directory.path(),
            &request,
            &baseline,
            &policy,
            &pin,
            &forged,
            now
        )
        .is_err()
    );
    forged = envelope.clone();
    forged.payload = STANDARD.encode(b"{}");
    assert!(
        ci_validate_reviewed(
            directory.path(),
            &request,
            &baseline,
            &policy,
            &pin,
            &forged,
            now
        )
        .is_err()
    );
    let mut missing_reviewer = policy.clone();
    missing_reviewer.required_reviewers.push(ContractReviewer {
        id: "second".into(),
        public_key: STANDARD.encode(SigningKey::from_bytes(&[44; 32]).verifying_key().to_bytes()),
        ..policy.required_reviewers[0].clone()
    });
    assert!(
        ci_validate_reviewed(
            directory.path(),
            &request,
            &baseline,
            &missing_reviewer,
            &missing_reviewer.fingerprint().unwrap(),
            &envelope,
            now
        )
        .is_err()
    );
    let second_key = SigningKey::from_bytes(&[44; 32]);
    let mut both = envelope.clone();
    both.signatures.push(ReportSignature {
        keyid: None,
        sig: STANDARD.encode(second_key.sign(&message).to_bytes()),
    });
    let two = ci_validate_reviewed(
        directory.path(),
        &request,
        &baseline,
        &missing_reviewer,
        &missing_reviewer.fingerprint().unwrap(),
        &both,
        now,
    )
    .unwrap();
    assert!(two.valid);
    assert_eq!(two.admission.reviewers, ["maintainer", "second"]);
    let mut alias = policy.clone();
    alias.required_reviewers.push(ContractReviewer {
        id: "alias".into(),
        ..policy.required_reviewers[0].clone()
    });
    assert!(alias.fingerprint().is_err());
    for altered in [
        CiContractApproval {
            decision: ContractReviewDecision::RequestChanges,
            ..approval.clone()
        },
        CiContractApproval {
            reviewed_at: now + chrono::Duration::minutes(1),
            ..approval.clone()
        },
        CiContractApproval {
            head_contract: ContentHash::of(b"other contract"),
            ..approval.clone()
        },
    ] {
        let bytes = serde_json::to_vec(&altered).unwrap();
        let mut pae = format!(
            "DSSEv1 {} {} {} ",
            CONTRACT_REVIEW_PAYLOAD_TYPE.len(),
            CONTRACT_REVIEW_PAYLOAD_TYPE,
            bytes.len()
        )
        .into_bytes();
        pae.extend_from_slice(&bytes);
        let signed = SignedContractReview {
            payload: STANDARD.encode(bytes),
            signatures: vec![ReportSignature {
                keyid: None,
                sig: STANDARD.encode(signing.sign(&pae).to_bytes()),
            }],
            ..envelope.clone()
        };
        assert!(
            ci_validate_reviewed(
                directory.path(),
                &request,
                &baseline,
                &policy,
                &pin,
                &signed,
                now
            )
            .is_err()
        );
    }
    let reviewed = ci_validate_reviewed(
        directory.path(),
        &request,
        &baseline,
        &policy,
        &pin,
        &envelope,
        now,
    )
    .unwrap();
    assert!(reviewed.valid);
    assert!(!reviewed.validation.valid); // The underlying unreviewed result is retained.
    assert_eq!(reviewed.admission.reviewers, ["maintainer"]);
    let import_input = ImportContractReviewRequest {
        envelope: format!(" \n{}\n ", serde_json::to_string_pretty(&envelope).unwrap()),
        policy: policy.clone(),
        expected_policy: pin.clone(),
        baseline: baseline.clone(),
        expected_commit: reviewed.validation.head.commit.clone(),
        actor: "importer".into(),
    };
    let import_request = RequestId::new();
    let receipt = repository
        .import_contract_review(&import_input, &import_request)
        .unwrap();
    let retained: ImportedContractReviewRecord =
        serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(retained.record.input.envelope, import_input.envelope);
    assert_eq!(
        repository
            .import_contract_review(&import_input, &import_request)
            .unwrap(),
        receipt
    );
    assert_eq!(
        repository.imported_contract_reviews().unwrap(),
        std::slice::from_ref(&retained)
    );
    assert!(repository.doctor().unwrap().valid);
    let current = repository
        .reauthenticate_contract_review(
            &retained.record.id,
            &policy,
            &pin,
            &baseline,
            &import_input.expected_commit,
        )
        .unwrap();
    assert!(current.valid);
    assert_eq!(current.admission.approval, approval);
    let coverage_request = ReviewCoverageRequest {
        working_tree: false,
        revision: CiRevision::Head {},
        subject: Some(CiSubjectIdentity::Issue {
            id: contextual_issue.metadata.id.clone(),
        }),
        expected_subject: None,
        authority: None,
    };
    let coverage = repository
        .contract_review_coverage(&coverage_request)
        .unwrap();
    assert_eq!(coverage.rows.len(), 1);
    assert_eq!(coverage.rows[0].state, ReviewCoverageState::HistoricalMatch);
    assert!(!coverage.authenticated);
    let authenticated_request = ReviewCoverageRequest {
        authority: Some(ReviewCoverageAuthority {
            baseline: baseline.clone(),
            policy: policy.clone(),
            expected_policy: pin.clone(),
        }),
        ..coverage_request.clone()
    };
    let coverage = repository
        .contract_review_coverage(&authenticated_request)
        .unwrap();
    assert!(coverage.authenticated);
    assert_eq!(coverage.rows[0].state, ReviewCoverageState::Authenticated);
    let dirty_request = ReviewCoverageRequest {
        expected_subject: Some(ContentHash::of(b"dirty selected issue")),
        ..authenticated_request.clone()
    };
    let dirty = repository.contract_review_coverage(&dirty_request).unwrap();
    assert!(!dirty.authenticated);
    assert_eq!(dirty.rows[0].state, ReviewCoverageState::Stale);
    let context = repository
        .context(&ContextRequest::new(
            contextual_issue.metadata.id.as_str(),
            64 * 1024,
        ))
        .unwrap();
    let review_section = context
        .sections
        .iter()
        .find(|s| s.kind == ContextSectionKind::Reviews)
        .unwrap();
    assert!(review_section.entries.iter().any(|e| matches!(&e.content,
        ContextContent::ContractReview { summary, .. } if summary.state == ReviewCoverageState::HistoricalMatch && summary.current_reviewers.is_empty())));
    let stable = repository
        .context(&ContextRequest::new(
            contextual_issue.metadata.id.as_str(),
            64 * 1024,
        ))
        .unwrap();
    assert_eq!(context.anchor.fingerprint, stable.anchor.fingerprint);

    review_working_tree::exercise(
        &repository,
        directory.path(),
        &authenticated_request,
        &contextual_issue.metadata.id,
    );
    let snapshot = repository.export_snapshot().unwrap();
    snapshot.validate().unwrap();
    assert!(
        snapshot
            .files
            .iter()
            .any(|file| file.kind == SnapshotKind::ContractReview && file.path == retained.path)
    );
    let restored_root = tempfile::tempdir().unwrap();
    let destination = restored_root.path().join(".workdeck");
    let preview = preview_snapshot_restore(&destination, &snapshot).unwrap();
    assert!(preview.allowed, "{:?}", preview.blockers);
    restore_snapshot(
        &destination,
        &snapshot,
        Some(&preview.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    let restored = Repository::open_source(&destination).unwrap();
    assert_eq!(
        restored
            .imported_contract_review(&retained.record.id)
            .unwrap(),
        retained
    );
    // Historical proof survives transport; current admission also needs immutable Git objects.
    assert!(
        restored
            .reauthenticate_contract_review(
                &retained.record.id,
                &policy,
                &pin,
                &baseline,
                &import_input.expected_commit
            )
            .is_err()
    );
    assert!(restored.doctor().unwrap().valid);
    let mut changed_input = import_input.clone();
    changed_input.actor = "another-importer".into();
    assert_eq!(
        repository
            .import_contract_review(&changed_input, &import_request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let before = repository.operation_history().unwrap();
    changed_input.expected_policy = ContentHash::of(b"wrong policy");
    assert!(
        repository
            .import_contract_review(&changed_input, &RequestId::new())
            .is_err()
    );
    assert_eq!(repository.operation_history().unwrap(), before);
    for point in [
        transactions::FaultPoint::BeforeJournal,
        transactions::FaultPoint::AfterJournal,
        transactions::FaultPoint::AfterChange(0),
        transactions::FaultPoint::BeforeReceipt,
        transactions::FaultPoint::AfterReceipt,
    ] {
        let request = RequestId::new();
        let before = repository.imported_contract_reviews().unwrap().len();
        assert!(
            repository
                .import_contract_review_with_faults(&import_input, &request, |actual| {
                    if actual == point {
                        Err(PmError::new(ErrorCode::Io, "interrupted review import"))
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        repository.recover_operations().unwrap();
        let receipt = repository
            .import_contract_review(&import_input, &request)
            .unwrap();
        assert_eq!(
            repository
                .import_contract_review(&import_input, &request)
                .unwrap(),
            receipt
        );
        assert_eq!(
            repository.imported_contract_reviews().unwrap().len(),
            before + 1
        );
    }
    let before = repository.imported_contract_reviews().unwrap().len();
    let concurrent_request = RequestId::new();
    let barrier = std::sync::Barrier::new(2);
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            repository.import_contract_review(&import_input, &concurrent_request)
        });
        let second = scope.spawn(|| {
            barrier.wait();
            repository.import_contract_review(&import_input, &concurrent_request)
        });
        (
            first.join().unwrap().unwrap(),
            second.join().unwrap().unwrap(),
        )
    });
    assert_eq!(first, second);
    assert_eq!(
        repository.imported_contract_reviews().unwrap().len(),
        before + 1
    );
    // Deletion, byte edits and missing receipt each fail historical doctor/read admission.
    let path = repository.root().join(&retained.path);
    let original = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(!repository.doctor().unwrap().valid);
    assert_eq!(
        repository.imported_contract_reviews().unwrap_err().path,
        Some(retained.path.to_string_lossy().into_owned())
    );
    fs::write(&path, [&original[..], b"\n"].concat()).unwrap();
    assert!(!repository.doctor().unwrap().valid);
    fs::write(&path, &original).unwrap();
    let receipt_path = repository
        .root()
        .join("operations")
        .join(format!("{}.yml", receipt.operation_id));
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    fs::remove_file(&receipt_path).unwrap();
    assert!(!repository.doctor().unwrap().valid);
    fs::write(&receipt_path, receipt_bytes).unwrap();
    assert!(repository.doctor().unwrap().valid);
    use workdeck_pm::projection::*;
    let mut projection = ProjectionStore::open(
        directory.path(),
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap();
    let ProjectionRefresh::Published(view) = projection
        .refresh(&ProjectionRefreshRequest::default())
        .unwrap()
    else {
        panic!("projection required")
    };
    let query = view
        .query(&ProjectionQuery::Activity {
            query: ProjectionActivityQuery {
                kinds: vec![SnapshotKind::ContractReview],
                after: Some(retained.record.imported_at - chrono::Duration::seconds(1)),
                ..Default::default()
            },
        })
        .unwrap();
    let page = view.page(&query, 0, 20).unwrap();
    assert_eq!(page.rows.len(), 7);
    assert!(
        page.rows
            .iter()
            .all(|row| row.assignee.as_deref() == Some("importer")
                && row.status.as_deref() == Some("approved"))
    );

    assert!(
        ci_validate_reviewed(
            directory.path(),
            &request,
            &baseline,
            &policy,
            &ContentHash::of(b"other policy"),
            &envelope,
            now
        )
        .is_err()
    );
    assert!(
        ci_validate_reviewed(
            directory.path(),
            &request,
            &baseline,
            &policy,
            &pin,
            &envelope,
            now + chrono::Duration::hours(2)
        )
        .is_err()
    );
    let raced = repository
        .context_with_faults(
            &ContextRequest::new(contextual_issue.metadata.id.as_str(), 64 * 1024),
            |_| {
                fs::write(
                    directory.path().join("application.txt"),
                    "new application revision\n",
                )
                .unwrap();
                git(directory.path(), &["add", "--", "application.txt"]);
                git(
                    directory.path(),
                    &[
                        "commit",
                        "--quiet",
                        "--no-gpg-sign",
                        "-m",
                        "move HEAD during context",
                    ],
                );
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(raced.code, ErrorCode::StaleSource);
    assert!(
        raced.message.contains("Contract reviews"),
        "{}",
        raced.message
    );
    create(&repository, "Later candidate");
    commit(directory.path());
    let later = CiValidateRequest {
        head: revision(directory.path()),
        ..request
    };
    assert!(
        ci_validate_reviewed(
            directory.path(),
            &later,
            &baseline,
            &policy,
            &pin,
            &envelope,
            now
        )
        .is_err()
    );

    let mut schema =
        serde_json::to_value(repository.organization_schema().unwrap().definition).unwrap();
    schema["fields"] =
        serde_json::json!({"risk": {"type":"text", "scopes":["issue"], "required":true}});
    write_definition(&repository, "schema.yml", &schema);
    commit(directory.path());
    let stale_coverage = repository
        .contract_review_coverage(&coverage_request)
        .unwrap();
    assert!(!stale_coverage.authenticated);
    assert!(
        stale_coverage
            .rows
            .iter()
            .all(|r| r.state != ReviewCoverageState::Authenticated)
    );
    let invalid_request = CiValidateRequest {
        head: revision(directory.path()),
        ..later
    };
    let invalid = ci_validate_pinned(directory.path(), &invalid_request, &baseline).unwrap();
    assert!(!invalid.head_report.policy_compliant);
    let approval = CiContractApproval {
        head: invalid.head,
        head_contract: invalid.contracts.head.unwrap().fingerprint,
        ..approval
    };
    let bytes = serde_json::to_vec(&approval).unwrap();
    let mut pae = format!(
        "DSSEv1 {} {} {} ",
        CONTRACT_REVIEW_PAYLOAD_TYPE.len(),
        CONTRACT_REVIEW_PAYLOAD_TYPE,
        bytes.len()
    )
    .into_bytes();
    pae.extend_from_slice(&bytes);
    let signed = SignedContractReview {
        payload: STANDARD.encode(bytes),
        signatures: vec![ReportSignature {
            keyid: None,
            sig: STANDARD.encode(signing.sign(&pae).to_bytes()),
        }],
        ..envelope
    };
    let invalid = ci_validate_reviewed(
        directory.path(),
        &invalid_request,
        &baseline,
        &policy,
        &pin,
        &signed,
        now,
    )
    .unwrap();
    assert!(
        !invalid.valid,
        "authenticated approval must not waive organization data violations"
    );
}

#[path = "ci_validation/review_working_tree.rs"]
mod review_working_tree;
