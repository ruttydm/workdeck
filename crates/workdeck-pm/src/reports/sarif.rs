//! Bounded SARIF 2.1.0 subset. External property files are never fetched.
use super::*;
use crate::SarifLevel;
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;

struct StrictJson(Value);
impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = StrictJson;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("JSON without duplicate object properties")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|n| StrictJson(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(value.into()))
            }
            fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<StrictJson>()? {
                    values.push(value.0);
                }
                Ok(StrictJson(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate JSON property"));
                    }
                    values.insert(key, map.next_value::<StrictJson>()?.0);
                }
                Ok(StrictJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}

fn level(value: &str) -> Option<SarifLevel> {
    match value {
        "error" => Some(SarifLevel::Error),
        "warning" => Some(SarifLevel::Warning),
        "note" => Some(SarifLevel::Note),
        "none" => Some(SarifLevel::None),
        _ => None,
    }
}

fn message(value: &Value) -> Option<&str> {
    value
        .get("text")
        .and_then(Value::as_str)
        .or_else(|| value.get("markdown").and_then(Value::as_str))
}

fn parse(
    bytes: &[u8],
    tool: &str,
    minimum: u64,
    levels: &[SarifLevel],
    result: &mut ReportAssessment,
) -> std::result::Result<(), &'static str> {
    let root: StrictJson = serde_json::from_slice(bytes).map_err(|_| "sarif_malformed_json")?;
    let root = root.0;
    if root.get("version").and_then(Value::as_str) != Some("2.1.0") {
        return Err("sarif_unsupported_version");
    }
    let runs = root
        .get("runs")
        .and_then(Value::as_array)
        .ok_or("sarif_runs_missing")?;
    if runs.len() > 1024 {
        return Err("sarif_run_limit");
    }
    let mut matched = 0;
    let mut invocation_failed = false;
    for run in runs {
        let driver = run
            .get("tool")
            .and_then(|v| v.get("driver"))
            .ok_or("sarif_driver_missing")?;
        let name = driver
            .get("name")
            .and_then(Value::as_str)
            .ok_or("sarif_driver_missing")?;
        if name != tool {
            continue;
        }
        matched += 1;
        if run
            .get("externalPropertyFileReferences")
            .is_some_and(|v| !v.as_object().is_some_and(|v| v.is_empty()))
        {
            return Err("sarif_external_properties_unavailable");
        }
        let invocations = run
            .get("invocations")
            .and_then(Value::as_array)
            .ok_or("sarif_invocations_missing")?;
        if invocations.is_empty() {
            return Err("sarif_invocations_missing");
        }
        for invocation in invocations {
            let success = invocation
                .get("executionSuccessful")
                .and_then(Value::as_bool)
                .ok_or("sarif_invocation_status_missing")?;
            result.counts.invocations += 1;
            invocation_failed |= !success;
            // Incomplete analysis is never inferred complete from a zero findings count.
            for key in [
                "toolExecutionNotifications",
                "toolConfigurationNotifications",
            ] {
                if invocation.get(key).is_some_and(|v| !v.is_array()) {
                    return Err("sarif_invalid_notifications");
                }
                if let Some(notifications) = invocation.get(key).and_then(Value::as_array) {
                    for notification in notifications {
                        if !notification.is_object() {
                            return Err("sarif_invalid_notifications");
                        }
                        let notification_level = match notification.get("level") {
                            Some(value) => {
                                level(value.as_str().ok_or("sarif_invalid_notifications")?)
                                    .ok_or("sarif_invalid_notifications")?
                            }
                            None => SarifLevel::Warning,
                        };
                        if notification_level == SarifLevel::Error {
                            invocation_failed = true;
                        }
                    }
                }
            }
        }
        let findings = run
            .get("results")
            .and_then(Value::as_array)
            .ok_or("sarif_results_missing")?;
        if findings.len() > 100_000 {
            return Err("sarif_finding_limit");
        }
        for finding in findings {
            if !finding.is_object()
                || finding.get("kind").is_some_and(|v| !v.is_string())
                || finding.get("level").is_some_and(|v| !v.is_string())
                || finding.get("ruleId").is_some_and(|v| !v.is_string())
            {
                return Err("sarif_invalid_result");
            }
            let kind = finding
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("fail");
            if !matches!(
                kind,
                "fail" | "review" | "open" | "pass" | "notApplicable" | "informational"
            ) {
                return Err("sarif_unsupported_result_kind");
            }
            let rules = driver.get("rules").and_then(Value::as_array);
            let indexed = finding
                .get("ruleIndex")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|index| rules.and_then(|rules| rules.get(index)));
            if finding.get("ruleIndex").is_some() && indexed.is_none() {
                return Err("sarif_rule_reference_unresolved");
            }
            let rule_id = finding.get("ruleId").and_then(Value::as_str);
            if let (Some(descriptor), Some(id)) = (indexed, rule_id)
                && descriptor.get("id").and_then(Value::as_str) != Some(id)
            {
                return Err("sarif_rule_identity_mismatch");
            }
            let descriptor = indexed.or_else(|| {
                rule_id.and_then(|id| {
                    rules.and_then(|rules| {
                        rules
                            .iter()
                            .find(|rule| rule.get("id").and_then(Value::as_str) == Some(id))
                    })
                })
            });
            let result_level = finding
                .get("level")
                .and_then(Value::as_str)
                .or_else(|| {
                    descriptor
                        .and_then(|rule| rule.get("defaultConfiguration"))
                        .and_then(|v| v.get("level"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("warning");
            let result_level = level(result_level).ok_or("sarif_invalid_level")?;
            let text = finding
                .get("message")
                .and_then(message)
                .ok_or("sarif_message_unresolved")?;
            result.counts.findings += 1;
            if matches!(kind, "pass" | "notApplicable" | "informational")
                || !levels.contains(&result_level)
            {
                continue;
            }
            result.counts.failed += 1;
            let id = finding
                .get("ruleId")
                .and_then(Value::as_str)
                .or_else(|| {
                    descriptor
                        .and_then(|rule| rule.get("id"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("finding");
            let physical = finding
                .get("locations")
                .and_then(Value::as_array)
                .and_then(|locations| locations.first())
                .and_then(|location| location.get("physicalLocation"));
            let location = physical.and_then(|p| p.get("artifactLocation"));
            let path = location
                .and_then(|l| l.get("uri"))
                .and_then(Value::as_str)
                .or_else(|| {
                    location
                        .and_then(|l| l.get("index"))
                        .and_then(Value::as_u64)
                        .and_then(|n| usize::try_from(n).ok())
                        .and_then(|index| {
                            run.get("artifacts")
                                .and_then(Value::as_array)
                                .and_then(|values| values.get(index))
                        })
                        .and_then(|artifact| artifact.get("location"))
                        .and_then(|location| location.get("uri"))
                        .and_then(Value::as_str)
                });
            let line = physical
                .and_then(|p| p.get("region"))
                .and_then(|r| r.get("startLine"))
                .and_then(Value::as_u64);
            result.failure(id, text, path, line);
        }
    }
    if matched == 0 {
        return Err("sarif_expected_tool_missing");
    }
    if result.counts.invocations < minimum {
        return Err("sarif_minimum_invocations_unmet");
    }
    result.state = if invocation_failed || result.counts.failed > 0 {
        ReportState::Failed
    } else {
        ReportState::Passed
    };
    result.reason_codes = vec![
        if invocation_failed {
            "sarif_tool_execution_failed"
        } else if result.counts.failed > 0 {
            "sarif_findings_failed"
        } else {
            "sarif_expected_tool_passed"
        }
        .into(),
    ];
    Ok(())
}

pub(super) fn assess(
    bytes: &[u8],
    tool: &str,
    minimum: u64,
    levels: &[SarifLevel],
) -> ReportAssessment {
    let mut result = ReportAssessment::new(ReportState::Unknown, "sarif_unassessed");
    if tool.is_empty() || minimum == 0 || levels.is_empty() {
        result.reason_codes = vec!["sarif_invalid_expectation".into()];
        return result;
    }
    if let Err(reason) = parse(bytes, tool, minimum, levels, &mut result) {
        result.state = ReportState::Unknown;
        result.reason_codes = vec![reason.into()];
    }
    result
}
