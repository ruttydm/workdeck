//! Portable local-feedback reports. Consistency is not producer authentication.
use super::{records, *};
use crate::{transactions::MutationReceipt, *};
use serde::{Deserialize, Serialize};

pub const MAX_CHECK_REPORT_BYTES: usize = 64 * 1024 * 1024;

/// Exported observations retain historical results and current freshness separately.
/// Receipts and hashes prove internal consistency, not a trusted producer identity.
/// Logs and artifact bodies are referenced by hash; they are not embedded here.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckReport {
    pub schema: SchemaVersion,
    pub basis: VerificationBasis,
    pub publication: RunPublication,
    pub observation: LocalResultAssessment,
    /// When the exporting process completed its freshness observation; not authenticated.
    pub observed_at: Timestamp,
    pub receipts: Vec<MutationReceipt>,
    pub fingerprint: ContentHash,
}
impl CheckReport {
    /// Parse and validate a bounded transport document without consulting a checkout.
    /// Successful validation grants no execution, freshness or completion authority.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_CHECK_REPORT_BYTES {
            return Err(invalid("check report exceeds 64 MiB"));
        }
        let report: Self = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
        report.validate()?;
        Ok(report)
    }

    fn content(&self) -> Result<ContentHash> {
        let value = serde_json::json!({
            "schema":self.schema, "basis":self.basis,
            "publication":self.publication, "observation":self.observation, "observed_at":self.observed_at,
            "receipts":self.receipts,
        });
        if serde_json::to_vec(&value)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > MAX_CHECK_REPORT_BYTES - 128
        {
            return Err(invalid("check report exceeds 64 MiB"));
        }
        crate::transactions::canonical_hash(&value)
    }

    pub fn validate(&self) -> Result<()> {
        if self.fingerprint != self.content()? {
            return Err(invalid("check report fingerprint differs from its content"));
        }
        if serde_json::to_vec_pretty(self)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > MAX_CHECK_REPORT_BYTES
        {
            return Err(invalid("check report exceeds 64 MiB"));
        }
        let run = &self.publication.intent;
        let result = &self.publication.result;
        if run.document.len() > MAX_RUN_RECORD_BYTES || result.document.len() > MAX_RUN_RECORD_BYTES
        {
            return Err(invalid("check report contains an oversized run record"));
        }
        records::validate_pair(run, result)?;
        if self.receipts.len() != 2
            || self.receipts[0].operation != records::RESERVE
            || self.receipts[1].operation != records::FINISH
        {
            return Err(invalid(
                "check report requires reservation and publication receipts",
            ));
        }
        for receipt in &self.receipts {
            records::validate_receipt(receipt)?;
        }
        if self.receipts[0].result != records::json_value(run)?
            || self.receipts[1].result != records::json_value(&self.publication)?
        {
            return Err(invalid("check report receipts belong to different records"));
        }
        if self.observation.run != run.intent.id
            || self.observation.historical_state != result.result.state
            || self.observation.basis != "local_feedback"
            || (self.observation.state != result.result.state
                && !matches!(self.observation.state, RunState::Stale | RunState::Unknown))
            || (self.observation.state != result.result.state
                && self.observation.reason_codes.is_empty())
        {
            return Err(invalid(
                "check report observation contradicts its retained result",
            ));
        }
        Ok(())
    }
}
impl Repository {
    /// Export a terminal run without executing, recovering or admitting evidence.
    pub fn export_check_report(&self, id: &LocalRunId) -> Result<CheckReport> {
        let outcome = self.check_status(id)?;
        let result = outcome.results.ok_or_else(|| {
            PmError::new(
                ErrorCode::PolicyBlocked,
                "a terminal published result is required for report export",
            )
            .details(serde_json::json!({"run":id,"state":outcome.state}))
        })?;
        let mut report = CheckReport {
            schema: SchemaVersion::CURRENT,
            basis: VerificationBasis::LocalFeedback,
            publication: RunPublication {
                intent: outcome.run,
                result,
            },
            observation: outcome.assessment,
            observed_at: chrono::Utc::now(),
            receipts: outcome.receipts,
            fingerprint: ContentHash::of(b""),
        };
        report.fingerprint = report.content()?;
        report.validate()?;
        Ok(report)
    }
}
