use workdeck_pm::{
    MAX_REPORT_BYTES, ReportExpectation, ReportState, SarifLevel, assess_check_report,
};

fn junit() -> ReportExpectation {
    ReportExpectation::JUnit {
        artifact: "tests".into(),
        suites: vec!["accepted".into()],
        minimum_tests: 1,
        maximum_skipped: None,
        allowed_exit_codes: vec![0],
    }
}

fn sarif() -> ReportExpectation {
    ReportExpectation::Sarif {
        artifact: "analysis".into(),
        tool: "fixture-analyzer".into(),
        minimum_invocations: 1,
        failure_levels: vec![SarifLevel::Error, SarifLevel::Warning],
        allowed_exit_codes: vec![0],
    }
}

#[test]
fn actual_expected_junit_cases_establish_report_success() {
    let report = br#"<testsuite name="accepted" tests="2" failures="0" errors="0" skipped="0"><testcase name="one" classname="Contract"/><testcase name="two" classname="Contract"/></testsuite>"#;
    let result = assess_check_report(&junit(), Some(report));
    assert_eq!(result.state, ReportState::Passed, "{result:?}");
    assert_eq!(result.counts.discovered, 2);
    assert_eq!(result.counts.passed, 2);
}

#[test]
fn junit_failures_empty_skipped_and_wrong_suites_cannot_pass() {
    for (report, expected) in [
        (
            r#"<testsuite name="accepted" tests="1" failures="1"><testcase name="fails"><failure message="expected result missing"/></testcase></testsuite>"#,
            ReportState::Failed,
        ),
        (
            r#"<testsuite name="accepted" tests="0"/>"#,
            ReportState::Unknown,
        ),
        (
            r#"<testsuite name="accepted" tests="1" skipped="1"><testcase name="ignored"><skipped/></testcase></testsuite>"#,
            ReportState::Skipped,
        ),
        (
            r#"<testsuite name="other" tests="1"><testcase name="unrelated"/></testsuite>"#,
            ReportState::Unknown,
        ),
        (
            r#"<testsuite name="accepted" tests="99"><testcase name="only-one"/></testsuite>"#,
            ReportState::Unknown,
        ),
        (
            r#"<testsuite name="accepted"><testcase name="duplicate"/><testcase name="duplicate"/></testsuite>"#,
            ReportState::Unknown,
        ),
    ] {
        let result = assess_check_report(&junit(), Some(report.as_bytes()));
        assert_eq!(result.state, expected, "{report}: {result:?}");
    }
}

#[test]
fn junit_requires_valid_bounded_inert_xml() {
    for report in [
        "<testsuite name='accepted'><testcase name='a'></testsuite>",
        "<testsuite name='accepted'><testcase name='a'/></testsuite><testsuite name='extra'/>",
        "<!DOCTYPE testsuite SYSTEM 'file:///private/never-read'><testsuite name='accepted'><testcase name='a'/></testsuite>",
        "<unrecognized><testcase name='a'/></unrecognized>",
        "<testsuite name='accepted'><testcase name='a' name='b'/></testsuite>",
    ] {
        assert_eq!(
            assess_check_report(&junit(), Some(report.as_bytes())).state,
            ReportState::Unknown,
            "{report}"
        );
    }
    assert_eq!(
        assess_check_report(&junit(), None).reason_codes,
        ["report_missing"]
    );
    assert_eq!(
        assess_check_report(&junit(), Some(&vec![b'x'; MAX_REPORT_BYTES + 1])).reason_codes,
        ["report_size_limit"]
    );
}

fn sarif_bytes(invocations: serde_json::Value, results: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "version":"2.1.0",
        "runs":[{"tool":{"driver":{"name":"fixture-analyzer"}}, "invocations":invocations,"results":results}]
    })).unwrap()
}

#[test]
fn sarif_requires_matching_completed_invocation_even_without_findings() {
    let valid = sarif_bytes(
        serde_json::json!([{"executionSuccessful":true}]),
        serde_json::json!([]),
    );
    let result = assess_check_report(&sarif(), Some(&valid));
    assert_eq!(result.state, ReportState::Passed, "{result:?}");
    assert_eq!(result.counts.invocations, 1);
    for (invocations, state) in [
        (serde_json::json!([]), ReportState::Unknown),
        (serde_json::json!([{}]), ReportState::Unknown),
        (
            serde_json::json!([{"executionSuccessful":false}]),
            ReportState::Failed,
        ),
    ] {
        let report = sarif_bytes(invocations, serde_json::json!([]));
        assert_eq!(assess_check_report(&sarif(), Some(&report)).state, state);
    }
    let wrong = String::from_utf8(valid)
        .unwrap()
        .replace("fixture-analyzer", "other-tool");
    assert_eq!(
        assess_check_report(&sarif(), Some(wrong.as_bytes())).state,
        ReportState::Unknown
    );
}

#[test]
fn sarif_reports_failure_locations_and_rejects_incomplete_external_results() {
    let report = sarif_bytes(
        serde_json::json!([{"executionSuccessful":true}]),
        serde_json::json!([{"ruleId":"contract","level":"error","message":{"text":"Missing invariant"},"locations":[{"physicalLocation":{"artifactLocation":{"uri":"src/lib.rs"},"region":{"startLine":12}}}]}]),
    );
    let result = assess_check_report(&sarif(), Some(&report));
    assert_eq!(result.state, ReportState::Failed, "{result:?}");
    assert_eq!(result.failures[0].path.as_deref(), Some("src/lib.rs"));
    assert_eq!(result.failures[0].line, Some(12));
    let mut external: serde_json::Value = serde_json::from_slice(&report).unwrap();
    external["runs"][0]["externalPropertyFileReferences"] =
        serde_json::json!({"results":[{"location":{"uri":"missing.sarif"}}]});
    let external = serde_json::to_vec(&external).unwrap();
    assert_eq!(
        assess_check_report(&sarif(), Some(&external)).state,
        ReportState::Unknown
    );
}

#[test]
fn failure_output_is_bounded_without_losing_failure_counts() {
    let cases = (0..300)
        .map(|i| {
            format!(
                "<testcase name='case-{i}'><failure message='{}'/></testcase>",
                "🐦".repeat(1024)
            )
        })
        .collect::<String>();
    let report =
        format!("<testsuite name='accepted' tests='300' failures='300'>{cases}</testsuite>");
    let result = assess_check_report(&junit(), Some(report.as_bytes()));
    assert_eq!(result.state, ReportState::Failed);
    assert_eq!(result.counts.failed, 300);
    assert_eq!(result.failures.len(), 32);
    assert_eq!(result.omitted_failures, 268);
    assert!(serde_json::to_vec(&result).unwrap().len() < 64 * 1024);
}

#[test]
fn unrelated_junit_suites_cannot_fill_the_expected_suite_minimum() {
    let mut expectation = junit();
    if let ReportExpectation::JUnit { minimum_tests, .. } = &mut expectation {
        *minimum_tests = 2;
    }
    let report = br#"<testsuites><testsuite name="accepted"><testcase name="one"/></testsuite><testsuite name="other"><testcase name="two"/><testcase name="three"/></testsuite></testsuites>"#;
    let result = assess_check_report(&expectation, Some(report));
    assert_eq!(result.state, ReportState::Unknown);
    assert_eq!(result.reason_codes, ["junit_minimum_tests_unmet"]);
}

#[test]
fn sarif_duplicate_properties_and_rule_disagreement_are_ambiguous() {
    let duplicate = br#"{"version":"2.1.0","runs":[{"tool":{"driver":{"name":"fixture-analyzer"}},"invocations":[{"executionSuccessful":false,"executionSuccessful":true}],"results":[]}]}"#;
    assert_eq!(
        assess_check_report(&sarif(), Some(duplicate)).state,
        ReportState::Unknown
    );
    let mut report: serde_json::Value = serde_json::from_slice(&sarif_bytes(
        serde_json::json!([{"executionSuccessful":true}]),
        serde_json::json!([{"ruleId":"accepted-rule","ruleIndex":0,"message":{"text":"finding"}}]),
    ))
    .unwrap();
    report["runs"][0]["tool"]["driver"]["rules"] =
        serde_json::json!([{"id":"other-rule","defaultConfiguration":{"level":"none"}}]);
    let bytes = serde_json::to_vec(&report).unwrap();
    assert_eq!(
        assess_check_report(&sarif(), Some(&bytes)).state,
        ReportState::Unknown
    );
}

#[test]
fn junit_rejects_duplicate_or_unsupported_xml_declarations() {
    for prefix in [
        "<?xml version='1.1'?>",
        "<?xml version='9.0'?>",
        "<?xml version='1.0'?><?xml version='1.0'?>",
    ] {
        let report =
            format!("{prefix}<testsuite name='accepted'><testcase name='one'/></testsuite>");
        assert_eq!(
            assess_check_report(&junit(), Some(report.as_bytes())).state,
            ReportState::Unknown,
            "{prefix}"
        );
    }
    let report=b"<?xml version='1.0' encoding='UTF-8'?><testsuite name='accepted'><testcase name='one'/></testsuite>";
    assert_eq!(
        assess_check_report(&junit(), Some(report)).state,
        ReportState::Passed
    );
}

#[test]
fn malformed_sarif_notifications_cannot_silently_report_clean_analysis() {
    for notification in [
        serde_json::json!(false),
        serde_json::json!({"level":7}),
        serde_json::json!({"level":"invalid"}),
    ] {
        let bytes = sarif_bytes(
            serde_json::json!([{"executionSuccessful":true,"toolExecutionNotifications":[notification]}]),
            serde_json::json!([]),
        );
        assert_eq!(
            assess_check_report(&sarif(), Some(&bytes)).state,
            ReportState::Unknown
        );
    }
}

#[test]
fn junit_root_aggregate_counts_cannot_contradict_actual_cases() {
    for attrs in [
        "tests='1' failures='1'",
        "tests='2'",
        "errors='1'",
        "skipped='1'",
        "tests='invalid'",
    ] {
        let report = format!(
            "<testsuites {attrs}><testsuite name='accepted' tests='1'><testcase name='one'/></testsuite></testsuites>"
        );
        assert_eq!(
            assess_check_report(&junit(), Some(report.as_bytes())).state,
            ReportState::Unknown,
            "{attrs}"
        );
    }
    let report=b"<testsuites tests='1' failures='0'><testsuite name='accepted' tests='1'><testcase name='one'/></testsuite></testsuites>";
    assert_eq!(
        assess_check_report(&junit(), Some(report)).state,
        ReportState::Passed
    );
}
