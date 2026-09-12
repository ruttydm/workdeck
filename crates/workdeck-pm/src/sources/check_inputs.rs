use super::{capture_core, git::BoundGit};
use crate::*;
use std::{collections::BTreeSet, path::Path};

pub(crate) fn bind_check_inputs(
    root: &Path,
    source: &CiSourceIdentity,
    plan: &CheckPlan,
) -> Result<Vec<CiInvocationInputManifest>> {
    let limits = SourceCaptureLimits::default();
    let git = BoundGit::with_shared_limits(root, &limits)?;
    if git.resolve_commit(&source.commit)?.as_ref() != Some(&source.commit)
        || git.tree(&source.commit)? != source.tree
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "CI source tree differs from commit",
        ));
    }
    let files = capture_core::blobs(&git, &git.tree_entries(&source.tree)?, &limits)?;
    let snapshot = crate::transactions::Snapshot::from_memory(Path::new("ci-check-source"), &files);
    let config = crate::repository::config_from_snapshot(Path::new("ci-check-source"), &snapshot)?;
    capture_core::validate_role(&files, &config, SourceRole::Proposal, None)?;
    let validation =
        crate::staged_doctor::inspect_source_files(&files, &config.repository, &limits)?;
    if !validation.valid || !validation.policy_compliant {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "committed planning is invalid for CI input admission",
        )
        .details(serde_json::json!({"validation":validation})));
    }
    let definitions = crate::commands::catalog::capture(&snapshot, &config)?;
    if source.repository != config.repository
        || source.repository != plan.repository
        || source.content != capture_core::content_hash(&files)?
        || plan.config
            != ContentHash::of(files.get(Path::new("config.yml")).ok_or_else(|| {
                PmError::new(ErrorCode::NotFound, "committed configuration missing")
            })?)
        || plan.definitions != definitions
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "check definitions or configuration differ from the committed source",
        ));
    }
    if let Some(subject) = &plan.issue {
        let issue = crate::issues::resolve_issue(
            Path::new("ci-check-source"),
            &snapshot,
            &config,
            subject.id.as_str(),
        )?;
        let requirements = crate::context::requirement_fingerprint(
            Path::new("ci-check-source"),
            &snapshot,
            &config,
            &subject.id,
        )?;
        if issue.source != subject.source || requirements != subject.requirements {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "check issue or inherited requirements differ from the committed revision",
            )
            .at(&issue.path));
        }
    }
    if plan.checks.is_empty() || !plan.blockers.is_empty() {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "CI input binding requires a nonempty unblocked check plan",
        ));
    }
    let mut selected = Vec::new();
    let mut absent = BTreeSet::new();
    for invocation in &plan.invocations {
        if !invocation.inputs.complete {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "CI input binding requires complete input manifests",
            ));
        }
        let mut check = definitions
            .checks
            .iter()
            .find(|record| record.definition.id == invocation.id)
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::InvalidInput,
                    "CI input binding requires check invocations",
                )
            })?
            .clone();
        let mut files: BTreeSet<_> = invocation
            .input_selection
            .files
            .iter()
            .chain(&invocation.input_selection.dependency_files)
            .chain(&invocation.input_selection.toolchain_files)
            .cloned()
            .collect();
        for entry in &invocation.inputs.entries {
            if let InputEntry::Absent { path } = entry {
                absent.insert(path.clone());
            }
        }
        files.extend(
            invocation
                .input_selection
                .optional_files
                .iter()
                .filter(|path| !absent.contains(*path))
                .cloned(),
        );
        for tool in &invocation.inputs.tools {
            if let Some(ToolLocation::Worktree { path }) = &tool.resolved {
                files.insert(path.clone());
            }
        }
        files.retain(|path| {
            !invocation
                .input_selection
                .trees
                .iter()
                .any(|tree| crate::ci_evaluators::covers(tree, path))
        });
        check.definition.evaluator_inputs = Some(EvaluatorInputSelection {
            files: files.into_iter().collect(),
            trees: invocation.input_selection.trees.clone(),
        });
        selected.push(check);
    }
    // Optional absence is itself source-bound. A committed file cannot disappear
    // from the denominator merely because the current checkout removed it.
    let absent: Vec<_> = absent.into_iter().collect();
    if absent.len() > 4096
        || absent.iter().any(|path| path.as_os_str().len() > 4096)
        || absent
            .iter()
            .map(|path| path.as_os_str().len())
            .sum::<usize>()
            > 1024 * 1024
    {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "optional CI input selection exceeds capture bounds",
        ));
    }
    for group in absent.chunks(8) {
        for entry in git.evaluator_tree_entries(&source.tree, group)? {
            if group.contains(&entry.path) {
                return Err(super::check_input_entries::stale().at(&entry.path));
            }
        }
    }
    let mut input_limits = limits.clone();
    input_limits.max_entries = limits.max_entries.saturating_sub(files.len());
    input_limits.max_total_bytes = limits
        .max_total_bytes
        .saturating_sub(files.values().map(Vec::len).sum());
    let inputs = super::evaluators::capture(&git, &source.tree, &selected, &input_limits)?;
    for (invocation, committed) in plan.invocations.iter().zip(&inputs) {
        let actual = super::check_input_entries::local(&invocation.inputs)?;
        let expected = super::check_input_entries::committed(committed);
        if let Some(path) = actual
            .keys()
            .chain(expected.keys())
            .find(|path| actual.get(*path) != expected.get(*path))
        {
            return Err(super::check_input_entries::stale()
                .at(path)
                .details(serde_json::json!({"invocation":invocation.id})));
        }
    }
    let manifests = plan
        .invocations
        .iter()
        .zip(inputs)
        .map(|(invocation, captured)| {
            let absent: Vec<_> = invocation
                .inputs
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    InputEntry::Absent { path } => Some(path.clone()),
                    _ => None,
                })
                .collect();
            let fingerprint = crate::transactions::canonical_hash(&serde_json::json!({
                "domain":"workdeck.ci-invocation-inputs.v1", "invocation":invocation.id,
                "selection":invocation.input_selection, "entries":captured.entries, "absent":absent,
            }))?;
            Ok(CiInvocationInputManifest {
                invocation: invocation.id.clone(),
                selection: invocation.input_selection.clone(),
                entries: captured.entries,
                absent,
                fingerprint,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    git.verify()?;
    Ok(manifests)
}
