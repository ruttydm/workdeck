use crate::*;
use serde::{Deserialize, Serialize};

#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct JUnitCaseIdentity {
    pub suites: Vec<String>,
    pub class_name: String,
    pub name: String,
}
impl JUnitCaseIdentity {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.suites.is_empty()
            || self.suites.len() > 32
            || self.suites.iter().any(|s| s.is_empty() || s.len() > 256)
            || self.class_name.len() > 1024
            || self.name.is_empty()
            || self.name.len() > 1024
        {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "JUnit case identity exceeds supported bounds",
            ));
        }
        Ok(())
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JUnitCaseOutcome {
    Passed,
    Failed,
    Error,
    Skipped,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JUnitTestCase {
    pub identity: JUnitCaseIdentity,
    pub outcome: JUnitCaseOutcome,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JUnitCaseReport {
    pub cases: Vec<JUnitTestCase>,
    pub fingerprint: ContentHash,
}
/// Inert case inventory, not producer or completion authority. Pairing consumers
/// must first bind the exact artifact bytes to an authenticated execution result.
pub fn junit_test_cases(bytes: &[u8]) -> Result<JUnitCaseReport> {
    if bytes.len() > super::MAX_REPORT_BYTES {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "JUnit report exceeds 4 MiB",
        ));
    }
    let mut cases = super::junit::inventory(bytes)
        .map_err(|reason| PmError::new(ErrorCode::InvalidInput, reason))?;
    cases.sort_by(|a, b| a.identity.cmp(&b.identity));
    if cases.is_empty()
        || cases
            .windows(2)
            .any(|pair| pair[0].identity == pair[1].identity)
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "JUnit case inventory is empty or ambiguous",
        ));
    }
    let fingerprint = crate::transactions::canonical_hash(
        &serde_json::json!({"domain":"workdeck.junit-cases.v1","cases":cases}),
    )?;
    Ok(JUnitCaseReport { cases, fingerprint })
}
