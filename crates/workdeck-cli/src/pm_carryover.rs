//! CLI intent adapter for the shared reviewed cycle carryover operation.
use super::pm_cli;
use clap::Args;
use serde_json::Value;
use workdeck_pm::{CycleCarryoverRequest, Repository, RequestId, Result};
#[derive(Debug, Args)]
pub(super) struct Input {
    pub from: String,
    pub to: String,
    #[arg(
        long,
        value_name = "ISSUE_ID",
        help = "Select an eligible source member; repeat for an explicit batch of at most 100"
    )]
    pub issue: Vec<String>,
    #[arg(
        long,
        value_name = "SHA256",
        help = "Apply this exact reviewed preview; omission previews without writing"
    )]
    pub expected_preview: Option<String>,
}
pub(super) fn run(
    repository: &Repository,
    source: &Value,
    options: &pm_cli::IssueOptions,
    input: &Input,
    json: bool,
) -> Result<()> {
    if options.expected_content.is_some() || options.expected_revision.is_some() {
        return Err(pm_cli::invalid(
            "carryover uses --expected-preview for the complete batch, not a single record token",
        ));
    }
    let request = CycleCarryoverRequest {
        from: input.from.clone(),
        to: input.to.clone(),
        issues: input
            .issue
            .iter()
            .map(|id| id.parse())
            .collect::<Result<Vec<_>>>()?,
    };
    match &input.expected_preview {
        None => {
            pm_cli::read_options(options)?;
            pm_cli::emit(
                json,
                "cycle_carryover_preview",
                source,
                &repository.preview_cycle_carryover(&request)?,
                None,
            )
        }
        Some(expected) => {
            let id = options
                .request_id
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or_else(RequestId::new);
            let receipt = repository.apply_cycle_carryover(&request, &expected.parse()?, &id)?;
            pm_cli::emit_mutation(
                repository,
                options,
                json,
                "cycle_carryover",
                source,
                &receipt,
            )
        }
    }
}
