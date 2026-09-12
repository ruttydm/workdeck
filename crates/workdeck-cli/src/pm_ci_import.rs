//! Durable signed-report storage and explicit current-policy reauthentication.
use super::{
    pm_ci::{CiCommand, CiOptions},
    pm_cli::{self, CommandFailure},
};
use serde_json::json;
use std::path::Path;
use workdeck_pm::*;

pub(super) fn run(cwd: &Path, options: &CiOptions, command: &CiCommand) -> anyhow::Result<()> {
    let mut source = json!({"repository":null,"root":cwd,"role":"imported_check_reports"});
    (|| -> workdeck_pm::Result<()> {
        let repository = Repository::open_source(&cwd.join(".workdeck"))?;
        source["repository"] = json!(repository.identity());
        match command {
            CiCommand::ImportReport { red_green_file, report_file, policy_file, expected_policy, expected_commit, actor, request_id } => {
                let envelope = pm_cli::read_regular_input(cwd, report_file, MAX_IMPORTED_REPORT_BYTES as u64)?;
                let policy = ProducerTrustPolicy::from_json(&pm_cli::read_regular_input(cwd, policy_file, MAX_PRODUCER_POLICY_BYTES as u64)?)?;
                let input = ImportCheckReportRequest {
                    red_green: red_green_file.as_ref().map(|path| {
                        let bytes = pm_cli::read_regular_input(cwd, path, MAX_IMPORTED_REPORT_BYTES as u64)?;
                        serde_json::from_slice::<RetainedRedGreenProof>(&bytes).map_err(|e| pm_cli::invalid(format!("invalid retained red/green proof: {e}")))
                    }).transpose()?,
                    envelope:String::from_utf8(envelope).map_err(|_| pm_cli::invalid("signed envelope must be UTF-8"))?,
                    policy, expected_policy:expected_policy.parse()?, expected_commit:expected_commit.parse()?, actor:actor.clone(),
                };
                let receipt = repository.import_check_report(&input, &request_id.parse()?)?;
                pm_cli::emit(options.json, "ci.import_report", &source, &receipt.result, Some(&receipt))
                    .map_err(|error| error.details(json!({"mutation_committed":true,"request_id":receipt.request_id,"receipt":receipt})))?;
            }
            CiCommand::Reports => pm_cli::emit(options.json, "ci.reports", &source, &repository.imported_check_report_summaries()?, None)?,
            CiCommand::Report { id } => pm_cli::emit(options.json, "ci.report", &source, &repository.imported_check_report(&id.parse()?)?, None)?,
            CiCommand::Reauthenticate { id, policy_file, expected_policy, expected_commit } => {
                let policy = ProducerTrustPolicy::from_json(&pm_cli::read_regular_input(cwd, policy_file, MAX_PRODUCER_POLICY_BYTES as u64)?)?;
                let admitted = repository.reauthenticate_imported_report(&id.parse()?, &policy, &expected_policy.parse()?, &expected_commit.parse()?)?;
                pm_cli::emit(options.json, "ci.reauthenticate", &source, &admitted, None)?;
            }
            CiCommand::ReauthenticateRedGreen { id, authority_file } => {
                let bytes = pm_cli::read_regular_input(cwd, authority_file, 1024*1024)?;
                let authority: RetainedRedGreenAuthority = serde_json::from_slice(&bytes).map_err(|e| pm_cli::invalid(format!("invalid red/green authority: {e}")))?;
                let assessment = repository.reauthenticate_imported_red_green(id, &authority)?;
                pm_cli::emit(options.json, "ci.reauthenticate_red_green", &source, &assessment, None)?;
            }
            CiCommand::VerifyImportedCheck { input } => {
                let bytes = pm_cli::read_regular_input(cwd, input, 2*1024*1024)?;
                let input: VerifyImportedCheck = serde_json::from_slice(&bytes).map_err(|e| pm_cli::invalid(format!("invalid imported check selection: {e}")))?;
                let assessment = repository.verify_imported_check(&input)?;
                pm_cli::emit(options.json, "ci.verify_imported_check", &source, &assessment, None)?;
            }
            _ => unreachable!("only imported report commands use this dispatch"),
        }
        Ok(())
    })().map_err(|error| CommandFailure { error, source_identity:source })?;
    Ok(())
}
