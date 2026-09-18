use workdeck_pm::*;

#[test]
fn red_green_case_capture_preserves_all_outcomes_and_unambiguous_identity() {
    let bytes = br#"<testsuites><testsuite name="outer"><testsuite name="inner"><testcase classname="A::B" name="C"/><testcase classname="A" name="B::C"><failure/></testcase><testcase name="setup"><error/></testcase><testcase name="later"><skipped/></testcase></testsuite></testsuite></testsuites>"#;
    let report = junit_test_cases(bytes).unwrap();
    assert_eq!(report.cases.len(), 4);
    assert_eq!(report.cases[0].identity.suites, ["outer", "inner"]);
    let states = report.cases.iter().map(|c| c.outcome).collect::<Vec<_>>();
    for state in [
        JUnitCaseOutcome::Passed,
        JUnitCaseOutcome::Failed,
        JUnitCaseOutcome::Error,
        JUnitCaseOutcome::Skipped,
    ] {
        assert!(states.contains(&state));
    }
    let reordered = br#"<testsuites><testsuite name="outer"><testsuite name="inner"><testcase name="later"><skipped/></testcase><testcase name="setup"><error/></testcase><testcase classname="A" name="B::C"><failure/></testcase><testcase classname="A::B" name="C"/></testsuite></testsuite></testsuites>"#;
    assert_eq!(report, junit_test_cases(reordered).unwrap());
}

#[test]
fn red_green_case_capture_rejects_ambiguity_and_invalid_reports() {
    for bytes in [
        r#"<testsuites><testsuite name="same"><testcase name="one"/></testsuite><testsuite name="same"><testcase name="one"/></testsuite></testsuites>"#,
        r#"<testsuite name="same" tests="2"><testcase name="one"/></testsuite>"#,
        r#"<!DOCTYPE a><testsuite name="same"><testcase name="one"/></testsuite>"#,
    ] {
        assert!(junit_test_cases(bytes.as_bytes()).is_err(), "{bytes}");
    }
}

#[test]
fn case_capture_limits_fail_without_returning_partial_coverage() {
    let oversized = format!(
        "<testsuite name=\"unit\"><testcase name=\"{}\"/></testsuite>",
        "x".repeat(1025)
    );
    assert!(junit_test_cases(oversized.as_bytes()).is_err());
    let mut report = String::from("<testsuite name=\"unit\">");
    for index in 0..20_001 {
        report.push_str(&format!("<testcase name=\"case{index}\"/>"));
    }
    report.push_str("</testsuite>");
    let error = junit_test_cases(report.as_bytes()).unwrap_err();
    assert!(error.message.contains("inventory_limit"));
}

#[test]
fn red_green_declarations_require_explicit_unambiguous_junit_assertions() {
    let case = serde_json::json!({"suites":["unit"],"class_name":"Behavior","name":"regression"});
    let value = serde_json::json!({"schema":1,"repository":RepositoryId::new(),"id":"unit","name":"Unit","command":"unit",
        "expectation":{"kind":"junit","artifact":"report","suites":["unit"],"minimum_tests":1,"allowed_exit_codes":[0]},
        "red_green":{"red_exit_codes":[1,101],"cases":[case.clone()]}});
    let definition: CheckDefinition = serde_json::from_value(value.clone()).unwrap();
    definition.validate().unwrap();
    for change in [
        serde_json::json!({"red_exit_codes":[],"cases":[case.clone()]}),
        serde_json::json!({"red_exit_codes":[256],"cases":[case.clone()]}),
        serde_json::json!({"red_exit_codes":[1],"cases":[case.clone(),case.clone()]}),
        serde_json::json!({"red_exit_codes":[1],"cases":[]}),
    ] {
        let mut changed = value.clone();
        changed["red_green"] = change;
        let definition: CheckDefinition = serde_json::from_value(changed).unwrap();
        assert_eq!(
            definition.validate().unwrap_err().code,
            ErrorCode::InvalidSchema
        );
    }
    let mut process = value;
    process["expectation"] = serde_json::json!({"kind":"process","allowed_exit_codes":[0]});
    let definition: CheckDefinition = serde_json::from_value(process).unwrap();
    assert_eq!(
        definition.validate().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
}
