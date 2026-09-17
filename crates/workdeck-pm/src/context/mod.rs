//! Snapshot-bound, budgeted agent context and explained advisory next actions.
mod budget;
mod capture;
mod check_runs;
mod external;
mod next;
mod packet;
mod requirements;
mod reviews;
mod types;
use crate::*;
pub(crate) use capture::{anchor_for_issue, subjects as issue_subjects};
pub use types::*;

/// Check planning binds authored requirements independently of run results,
/// continuity records and expanded source excerpts.
pub(crate) fn requirement_fingerprint(
    root: &std::path::Path,
    snapshot: &crate::transactions::Snapshot<'_>,
    config: &Config,
    issue: &IssueId,
) -> Result<ContentHash> {
    let captured = capture::Capture::load(root, snapshot, config.clone())?;
    let record = captured.resolve(issue.as_str())?;
    requirements::fingerprint(root, snapshot, &captured, record)
}

fn serialization(error: impl std::fmt::Display) -> PmError {
    PmError::new(ErrorCode::InvalidInput, error.to_string())
}
fn notice(code: &str, message: &str) -> ContextEntry {
    ContextEntry {
        content: ContextContent::Notice {
            reason_code: code.into(),
            message: message.into(),
        },
        citations: Vec::new(),
    }
}
fn check_expected(anchor: &ContextAnchor, expected: Option<&ContentHash>) -> Result<()> {
    if expected.is_some_and(|hash| hash != &anchor.fingerprint) {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "context inputs changed; capture current context before choosing an action",
        )
        .details(serde_json::json!({"current_context":anchor.fingerprint})));
    }
    Ok(())
}

impl Repository {
    pub fn context(&self, request: &ContextRequest) -> Result<ContextPacket> {
        self.context_with_faults(request, |_| Ok(()))
    }
    /// Qualification seam for direct-editor races; no production fault state is persisted.
    #[doc(hidden)]
    pub fn context_with_faults(
        &self,
        request: &ContextRequest,
        mut fault: impl FnMut(ContextFaultPoint) -> Result<()>,
    ) -> Result<ContextPacket> {
        if request.budget_bytes > MAX_CONTEXT_BUDGET_BYTES {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "context packet budget exceeds 1 MiB",
            ));
        }
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let captured = capture::Capture::load(self.root(), snapshot, config)?;
            let issue = captured.resolve(&request.issue)?;
            let questions = captured.applicability(self.root(), snapshot, issue)?;
            let links = captured.source_links(snapshot, issue)?;
            let mut external = external::External::new(self.root())?;
            let sources = external.sources(&links)?;
            let instructions = external.instructions(&links)?;
            let runs = check_runs::Runs::capture(
                self.root(),
                snapshot,
                &captured.config,
                &issue.metadata.id,
            )?;
            let reviews =
                reviews::Reviews::capture(self.root(), snapshot, &captured.config, issue)?;
            let anchor =
                captured.anchor(snapshot, issue, &questions, &external, &runs, &reviews)?;
            check_expected(&anchor, request.expected_context.as_ref())?;
            let packet = packet::packet(
                snapshot,
                &captured,
                issue,
                anchor,
                &questions,
                request,
                (sources, instructions, &runs, &reviews),
            )?;
            fault(ContextFaultPoint::BeforeSourceValidation)?;
            external.verify()?;
            runs.verify(self.root(), snapshot, &captured.config, &issue.metadata.id)?;
            reviews.verify(self.root(), snapshot, &captured.config, issue)?;
            Ok(packet)
        })
    }
    pub fn next_issue(&self, request: &NextIssueRequest) -> Result<NextIssueSelection> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let captured = capture::Capture::load(self.root(), snapshot, config)?;
            next::selection(self.root(), snapshot, &captured, request)
        })
    }
    pub fn next_actions(&self, request: &NextActionRequest) -> Result<NextActions> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let captured = capture::Capture::load(self.root(), snapshot, config)?;
            let issue = captured.resolve(&request.issue)?;
            let questions = captured.applicability(self.root(), snapshot, issue)?;
            let links = captured.source_links(snapshot, issue)?;
            let mut external = external::External::new(self.root())?;
            external.sources(&links)?;
            external.instructions(&links)?;
            let runs = check_runs::Runs::capture(
                self.root(),
                snapshot,
                &captured.config,
                &issue.metadata.id,
            )?;
            let reviews =
                reviews::Reviews::capture(self.root(), snapshot, &captured.config, issue)?;
            let anchor =
                captured.anchor(snapshot, issue, &questions, &external, &runs, &reviews)?;
            check_expected(&anchor, request.expected_context.as_ref())?;
            let actions = next::actions(&captured, issue, anchor, &questions)?;
            external.verify()?;
            runs.verify(self.root(), snapshot, &captured.config, &issue.metadata.id)?;
            reviews.verify(self.root(), snapshot, &captured.config, issue)?;
            Ok(actions)
        })
    }
}
