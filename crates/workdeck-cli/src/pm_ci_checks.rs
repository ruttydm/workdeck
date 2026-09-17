//! Revision-bound planning and durable foreground checks share the PM runner.
use super::{
    pm_ci::{CiCommand, CiOptions},
    pm_cli::{self, CommandFailure},
};
use serde_json::json;
use std::path::Path;
use workdeck_pm::*;

pub(super) fn run(cwd: &Path, options: &CiOptions, command: &CiCommand) -> anyhow::Result<()> {
    let mut source =
        json!({"repository":null,"root":cwd,"role":"ci_check","basis":"local_feedback"});
    let result = (|| -> workdeck_pm::Result<Option<u8>> {
        match command {
            CiCommand::Plan {
                revision,
                profile,
                issue,
            } => {
                let prepared = prepare_ci_check(
                    cwd,
                    &super::pm_ci::revision(revision)?,
                    &CheckPlanRequest {
                        profiles: vec![profile.clone()],
                        issue: issue.clone(),
                        ..Default::default()
                    },
                )?;
                source["repository"] = json!(prepared.binding.source.repository);
                source["revision"] = json!(prepared.binding.source);
                pm_cli::emit(options.json, "ci.plan", &source, &prepared, None)?;
                Ok(None)
            }
            CiCommand::Check {
                plan_file,
                expected_plan,
                actor,
                request_id,
            } => {
                let bytes =
                    pm_cli::read_regular_input(cwd, plan_file, MAX_RUN_RECORD_BYTES as u64)?;
                let mut value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| pm_cli::invalid(format!("invalid CI plan JSON: {e}")))?;
                if value.get("api_version").is_some() {
                    if value["api_version"] != 1
                        || value["ok"] != true
                        || value["kind"] != "ci.plan"
                    {
                        return Err(pm_cli::invalid(
                            "expected a successful ci.plan envelope or raw CiPreparedCheck",
                        ));
                    }
                    value = value["result"].take();
                }
                let prepared: CiPreparedCheck = serde_json::from_value(value)
                    .map_err(|e| pm_cli::invalid(format!("invalid CI plan: {e}")))?;
                source["repository"] = json!(prepared.binding.source.repository);
                source["revision"] = json!(prepared.binding.source);
                source["binding"] = json!(prepared.binding.fingerprint);
                let repository = Repository::open_source(&cwd.join(".workdeck"))?;
                let input = CiCheckRunRequest {
                    input: CheckRunRequest {
                        expected_plan: prepared.plan.fingerprint.clone(),
                        plan: prepared.plan,
                        actor: actor.clone(),
                    },
                    binding: prepared.binding,
                    expected_binding: expected_plan.parse()?,
                };
                let request = request_id.parse()?;
                let control = RunControl::default();
                let mut signals = super::register_process_signal_callback({
                    let control = control.clone();
                    move || {
                        if control.cancellation_requested() {
                            control.force_cancel();
                        } else {
                            control.cancel();
                        }
                    }
                })
                .map_err(|e| PmError::new(ErrorCode::Io, e))?;
                let result = repository.run_ci_check_plan(&input, &request, &control);
                signals.retire();
                let outcome = result?;
                pm_cli::emit(options.json, "ci.check", &source, &outcome, None).map_err(|e| {
                    e.details(
                        json!({"mutation_committed":true,"run_id":outcome.run.intent.id,
                        "request_id":outcome.run.intent.request_id,"receipts":outcome.receipts}),
                    )
                })?;
                Ok(match outcome.state {
                    RunState::Passed => None,
                    RunState::Canceled => Some(130),
                    RunState::Failed => Some(1),
                    RunState::Stale | RunState::Running => Some(4),
                    RunState::Unknown => Some(6),
                    RunState::Blocked | RunState::NotRun | RunState::Skipped => Some(5),
                })
            }
            CiCommand::RedGreen(_)
            | CiCommand::ReviewCoverage(_)
            | CiCommand::ImportReview(_)
            | CiCommand::Reviews
            | CiCommand::Review { .. }
            | CiCommand::ReauthenticateReview(_)
            | CiCommand::ValidateReviewed(_)
            | CiCommand::ReviewPolicy { .. }
            | CiCommand::ImportReport { .. }
            | CiCommand::Reports
            | CiCommand::Report { .. }
            | CiCommand::Reauthenticate { .. }
            | CiCommand::ReauthenticateRedGreen { .. }
            | CiCommand::VerifyImportedCheck { .. }
            | CiCommand::Policy { .. }
            | CiCommand::Authenticate { .. }
            | CiCommand::Validate { .. } => {
                unreachable!("validation uses its immutable dispatch")
            }
        }
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: source,
    })?;
    if let Some(code) = result {
        return Err(super::CommandExit(i32::from(code)).into());
    }
    Ok(())
}
