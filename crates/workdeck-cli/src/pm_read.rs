//! Native read-only command adapters composed over shared planning queries.
use super::pm_cli::{emit, invalid};
use serde_json::json;
use std::collections::BTreeSet;
use workdeck_cli::{
    config::Config,
    payload::{search_target_group, search_target_payload},
    repository_panels::RepositoryPanels,
};
use workdeck_pm::{ErrorCode, PmError, Repository};

pub(super) fn search(
    repository: &Repository,
    query: &str,
    targets: &[String],
    json_output: bool,
) -> workdeck_pm::Result<()> {
    let filters = targets
        .iter()
        .map(|target| target.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if let Some(invalid_target) = filters.iter().find(|target| {
        !matches!(
            target.as_str(),
            "files" | "changes" | "issues" | "agents" | "git"
        )
    }) {
        return Err(invalid(format!(
            "unknown search target {invalid_target:?}; expected files, changes, issues, agents, or git"
        )));
    }
    let root = repository
        .root()
        .parent()
        .ok_or_else(|| invalid("planning source has no repository root"))?;
    let config = Config::load(root)
        .map_err(|failure| invalid(format!("application configuration: {failure}")))?;
    let base = (!config.git.base_branch.is_empty()).then_some(config.git.base_branch);
    let provider = RepositoryPanels::new(root, base, config.git.recent_commits)
        .map_err(|failure| PmError::new(ErrorCode::Io, failure.message))?;
    let (results, truncated) = provider
        .search_matching(query, 100, |target| {
            filters.is_empty() || filters.contains(search_target_group(target))
        })
        .map_err(|failure| {
            // Keep authoritative parser/source diagnostics structured. Git or file
            // failures retain their own message when the planning source is valid.
            match repository.doctor() {
                Err(diagnostic) => diagnostic,
                Ok(report) if !report.valid => report
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| invalid("planning data failed validation")),
                Ok(_) => {
                    let mut diagnostic = PmError::new(ErrorCode::Io, failure.message);
                    diagnostic.hint = failure.hint;
                    diagnostic
                }
            }
        })?;
    let results = results.into_iter()
        .map(|result|json!({"score":result.score,"label":result.record.label,"detail":result.record.detail,"target":search_target_payload(&result.record.target)}))
        .collect::<Vec<_>>();
    // The adapter must not emit another repository's records with the original
    // envelope identity if config.yml was replaced during the scan.
    repository.config()?;
    let source = json!({"repository":repository.identity(),"root":repository.root()});
    if json_output {
        emit(
            true,
            "search_results",
            &source,
            &json!({"query":query,"results":results,"truncated":truncated,"coverage":"bounded_repository_scan"}),
            None,
        )
    } else {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let io_error = |failure| {
            PmError::new(
                ErrorCode::Io,
                format!("could not write search results: {failure}"),
            )
        };
        if results.is_empty() {
            writeln!(out, "no results").map_err(io_error)?;
        }
        for result in results {
            writeln!(
                out,
                "{:<6} {:<12} {}",
                result["score"].as_i64().unwrap_or_default(),
                result["target"]["kind"].as_str().unwrap_or("unknown"),
                workdeck_diff::sanitize_terminal_line(result["label"].as_str().unwrap_or_default())
            )
            .map_err(io_error)?;
        }
        if truncated {
            writeln!(
                out,
                "Results or scan coverage were limited; narrow the query for more specific matches."
            )
            .map_err(io_error)?;
        }
        Ok(())
    }
}
