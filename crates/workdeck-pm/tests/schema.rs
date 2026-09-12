use std::collections::BTreeMap;
use workdeck_pm::{
    AcceptanceCriterion, AcceptancePolicy, Config, ErrorCode, IssueMetadata, MetadataAuthority,
    Priority, SourceLink, Timestamp,
};

fn now() -> Timestamp {
    "2026-09-08T10:00:00Z".parse().unwrap()
}

fn issue() -> (Config, IssueMetadata) {
    let config = Config::new("WD").unwrap();
    let issue = IssueMetadata::new(&config, "Preserve the login destination", now()).unwrap();
    (config, issue)
}

#[test]
fn new_issue_uses_the_configured_initial_state_and_legacy_priority() {
    let mut config = Config::new("WD").unwrap();
    config.workflow.initial = "inbox".into();
    let issue = IssueMetadata::new(&config, "  Keep the original URL  ", now()).unwrap();
    assert_eq!(issue.title, "Keep the original URL");
    assert_eq!(issue.status, "inbox");
    assert_eq!(issue.priority, Priority::Medium);
    assert_eq!(issue.revision.get(), 1);
    assert_eq!(issue.created_at, issue.updated_at);
    assert!(issue.completed_at.is_none());
    assert!(issue.canceled_at.is_none());
    issue.validate(&config).unwrap();
}

#[test]
fn invalid_prefix_title_and_terminal_initial_state_are_rejected() {
    assert!(Config::new("workdeck").is_err());
    let mut config = Config::new("WD").unwrap();
    assert!(IssueMetadata::new(&config, "  ", now()).is_err());
    assert!(IssueMetadata::new(&config, "bad\nheading", now()).is_err());
    config.workflow.initial = "done".into();
    assert_eq!(
        config.validate().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
}

#[test]
fn issue_validation_observes_configured_workflow_and_timestamp_consistency() {
    let (config, mut issue) = issue();
    issue.status = "todo".into();
    assert_eq!(
        issue.validate(&config).unwrap_err().code,
        ErrorCode::InvalidSchema
    );
    issue.status = "done".into();
    assert!(
        issue.validate(&config).is_err(),
        "completion needs its timestamp"
    );
    issue.completed_at = Some(now());
    issue.validate(&config).unwrap();
    issue.canceled_at = Some(now());
    assert!(
        issue.validate(&config).is_err(),
        "completion is not cancellation"
    );
    issue.canceled_at = None;
    issue.completed_at = Some("2026-09-08T11:00:00Z".parse().unwrap());
    assert!(
        issue.validate(&config).is_err(),
        "completion cannot postdate update"
    );
    issue.completed_at = Some(now());
    issue.updated_at = "2026-09-08T09:00:00Z".parse().unwrap();
    assert!(
        issue.validate(&config).is_err(),
        "update cannot predate creation"
    );
}

#[test]
fn custom_and_namespaced_extensions_survive_typed_round_trips() {
    let input = include_str!("fixtures/custom-fields/issue.yml");
    let config = Config::new("WD").unwrap();
    let issue: IssueMetadata = serde_yaml_ng::from_str(input).unwrap();
    issue.validate(&config).unwrap();
    assert_eq!(issue.custom["risk"]["score"], 3);
    assert_eq!(issue.extra["x-import"]["source"], "legacy");
    let serialized = serde_yaml_ng::to_string(&issue).unwrap();
    let again: IssueMetadata = serde_yaml_ng::from_str(&serialized).unwrap();
    assert_eq!(again, issue);
    let mut config = config;
    config
        .extra
        .insert("x-team".into(), serde_json::json!({"name":"Résumé"}));
    config.custom.insert(
        "estimate_units".into(),
        serde_json::json!(["points", "hours"]),
    );
    config.validate().unwrap();
    let again: Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(again, config);
}

#[test]
fn unknown_typos_and_reserved_future_fields_fail_explicitly() {
    let (mut config, mut issue) = issue();
    issue
        .extra
        .insert("titel".into(), serde_json::json!("Typo"));
    let error = issue.validate(&config).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidSchema);
    assert!(error.message.contains("titel"));
    issue.extra.clear();
    issue
        .extra
        .insert("claim".into(), serde_json::json!("future-shared-claim"));
    assert_eq!(
        issue.validate(&config).unwrap_err().code,
        ErrorCode::Unsupported
    );
    config.extra.insert("workfow".into(), serde_json::json!({}));
    assert_eq!(
        config.validate().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
}

#[test]
fn malformed_fixtures_reject_typos_schema_versions_and_naive_timestamps() {
    let config = Config::new("WD").unwrap();
    let typo: IssueMetadata =
        serde_yaml_ng::from_str(include_str!("fixtures/malformed/typo.yml")).unwrap();
    assert_eq!(
        typo.validate(&config).unwrap_err().code,
        ErrorCode::InvalidSchema
    );
    assert!(
        serde_yaml_ng::from_str::<IssueMetadata>(include_str!(
            "fixtures/malformed/future-schema.yml"
        ))
        .is_err()
    );
    assert!(
        serde_yaml_ng::from_str::<IssueMetadata>(include_str!(
            "fixtures/malformed/naive-timestamp.yml"
        ))
        .is_err()
    );
}

#[test]
fn source_links_validate_portable_relative_paths_and_line_ranges() {
    for path in ["src/auth.rs", "docs/Résumé.md", "folder/file name.rs"] {
        SourceLink {
            path: path.into(),
            line: Some(1),
            end_line: Some(4),
        }
        .validate()
        .unwrap();
    }
    for path in [
        "",
        "/etc/passwd",
        "../secret",
        "src/../secret",
        "src/./auth.rs",
        "src//auth.rs",
        "C:/Windows/file",
        "C:\\Windows\\file",
        "\\\\server\\share",
        "src/NUL.txt",
        "src/aux",
        "src/foo.",
        "src/foo ",
        "src/a:b",
        "a\nfile",
        "a\0file",
    ] {
        let error = SourceLink {
            path: path.into(),
            line: None,
            end_line: None,
        }
        .validate()
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafePath, "{path:?}");
    }
    for (line, end_line) in [(Some(0), None), (Some(4), Some(3)), (None, Some(2))] {
        assert!(
            SourceLink {
                path: "src/lib.rs".into(),
                line,
                end_line
            }
            .validate()
            .is_err()
        );
    }
}

#[test]
fn source_shape_validation_does_not_follow_symlinks_or_require_files_to_exist() {
    SourceLink {
        path: "unavailable/checkouts/linked-file.rs".into(),
        line: None,
        end_line: None,
    }
    .validate()
    .unwrap();
}

#[test]
fn acceptance_criteria_require_unique_stable_ids_and_real_descriptions() {
    let (config, mut issue) = issue();
    issue.acceptance.push(AcceptanceCriterion {
        id: "login-deep-link".into(),
        description: "A deep link survives login".into(),
        checked: false,
    });
    issue.validate(&config).unwrap();
    issue.acceptance.push(issue.acceptance[0].clone());
    assert!(issue.validate(&config).is_err());
    issue.acceptance.pop();
    issue.acceptance[0].description = "  ".into();
    assert!(issue.validate(&config).is_err());
    issue.acceptance[0].description = "A deep link survives login".into();
    issue.acceptance[0].id = "../criterion".into();
    assert!(issue.validate(&config).is_err());
}

#[test]
fn future_check_requirements_are_preserved_but_cannot_satisfy_completion() {
    let policy: AcceptancePolicy = serde_yaml_ng::from_str(
        "required_checks: [auth-tests]\nrequired_profiles: [release-readiness]\n",
    )
    .unwrap();
    policy.validate().unwrap();
    assert_eq!(
        policy.ensure_supported_for_completion().unwrap_err().code,
        ErrorCode::Unsupported
    );
    AcceptancePolicy::default()
        .ensure_supported_for_completion()
        .unwrap();
    let mut config = Config::new("WD").unwrap();
    config.acceptance = policy;
    config.validate().unwrap();
    let serialized = serde_yaml_ng::to_string(&config).unwrap();
    assert_eq!(
        serde_yaml_ng::from_str::<Config>(&serialized).unwrap(),
        config
    );
}

#[test]
fn due_dates_allow_date_only_or_explicit_instants_without_guessing_timezones() {
    let (config, mut issue) = issue();
    for due in ["2026-09-09", "2026-09-09T08:00:00+02:00"] {
        issue.due_at = Some(due.into());
        issue.validate(&config).unwrap();
    }
    for due in ["tomorrow", "2026-02-30", "2026-09-09T08:00:00"] {
        issue.due_at = Some(due.into());
        assert!(issue.validate(&config).is_err(), "{due}");
    }
}

#[test]
fn minimal_fixture_and_declarative_policy_are_valid_without_new_functionality_claims() {
    let config: Config =
        serde_yaml_ng::from_str(include_str!("fixtures/minimal-repo/.workdeck/config.yml"))
            .unwrap();
    config.validate().unwrap();
    let raw = include_str!(
        "fixtures/minimal-repo/.workdeck/issues/WD-01ARZ3NDEKTSV4RRFFQ69G5FAV/item.md"
    );
    let frontmatter = raw
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---\n")
        .unwrap()
        .0;
    let issue: IssueMetadata = serde_yaml_ng::from_str(frontmatter).unwrap();
    issue.validate(&config).unwrap();
    assert!(config.acceptance.ensure_supported_for_completion().is_err());
    assert_eq!(issue.documents, ["docs/authentication.md"]);
    assert_eq!(issue.extra, BTreeMap::new());
}

#[test]
fn authority_classification_does_not_treat_claims_or_review_sessions_as_issue_fields() {
    assert_eq!(
        MetadataAuthority::for_issue_field("title"),
        Some(MetadataAuthority::Authoritative)
    );
    assert_eq!(
        MetadataAuthority::for_issue_field("last_activity_at"),
        Some(MetadataAuthority::Derived)
    );
    assert_eq!(
        MetadataAuthority::for_issue_field("selected_issue"),
        Some(MetadataAuthority::Local)
    );
    assert_eq!(
        MetadataAuthority::for_issue_field("claim"),
        Some(MetadataAuthority::Coordination)
    );
    assert_eq!(
        MetadataAuthority::for_issue_field("review_session"),
        Some(MetadataAuthority::ReviewReference)
    );
    assert_eq!(MetadataAuthority::for_issue_field("titel"), None);
}

#[test]
fn manual_acceptance_preserves_explicit_attribution_without_claiming_ci_evidence() {
    let (config, mut issue) = issue();
    issue.manual_acceptance = Some(
        serde_json::from_value(serde_json::json!({
            "actor": "reviewer-1",
            "reason": "Reviewed the expected redirect behavior",
            "accepted_at": "2026-09-08T10:00:00Z"
        }))
        .unwrap(),
    );
    issue.validate(&config).unwrap();
    let json = serde_json::to_value(&issue).unwrap();
    assert_eq!(json["manual_acceptance"]["actor"], "reviewer-1");
    assert!(json["manual_acceptance"].get("ci").is_none());
    let again: IssueMetadata = serde_json::from_value(json).unwrap();
    assert_eq!(again.manual_acceptance, issue.manual_acceptance);
    issue.manual_acceptance.as_mut().unwrap().actor.clear();
    assert!(issue.validate(&config).is_err());
    issue.manual_acceptance.as_mut().unwrap().actor = "reviewer-1".into();
    issue.manual_acceptance.as_mut().unwrap().reason = "  ".into();
    assert!(issue.validate(&config).is_err());
    issue.manual_acceptance.as_mut().unwrap().reason = "Reviewed".into();
    issue.manual_acceptance.as_mut().unwrap().accepted_at = "2026-09-08T11:00:00Z".parse().unwrap();
    assert!(issue.validate(&config).is_err());
}

#[test]
fn reference_shape_validation_rejects_duplicates_empty_actors_and_unsafe_links() {
    let (config, mut issue) = issue();
    issue.labels = vec!["bug".into(), "bug".into()];
    assert!(issue.validate(&config).is_err());
    issue.labels.pop();
    issue.assignee = Some("  ".into());
    assert!(issue.validate(&config).is_err());
    issue.assignee = Some("agent-1".into());
    issue.commits = vec!["HEAD~1".into(), "abcdef0123456789".into()];
    issue.documents = vec![
        "docs/decision.md".into(),
        "https://example.invalid/design".into(),
    ];
    issue.validate(&config).unwrap();
    issue.files = vec![SourceLink {
        path: "../outside.rs".into(),
        line: None,
        end_line: None,
    }];
    assert_eq!(
        issue.validate(&config).unwrap_err().code,
        ErrorCode::UnsafePath
    );
}

#[test]
fn future_policy_declarations_still_validate_their_own_shape() {
    for input in [
        "required_checks: [unit, unit]",
        "required_profiles: ['../release']",
        "required_checks: ['']",
    ] {
        let policy: AcceptancePolicy = serde_yaml_ng::from_str(input).unwrap();
        assert!(policy.validate().is_err(), "{input}");
    }
    assert!(serde_yaml_ng::from_str::<AcceptancePolicy>("require_all_criterai: true").is_err());
}

#[test]
fn extension_namespaces_do_not_permit_known_field_shadowing() {
    let (config, mut issue) = issue();
    for key in ["title", "x-", "x-Invalid", "x-../../outside"] {
        issue.extra.clear();
        issue.extra.insert(key.into(), serde_json::json!("shadow"));
        assert!(issue.validate(&config).is_err(), "{key}");
    }
}
