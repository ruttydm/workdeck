//! Offline consistency of retained revision pins; never establishes producer trust.
use super::check_input_entries;
use crate::*;
use std::{collections::BTreeSet, path::Path};

pub(crate) fn validate(binding: &CiCheckInputBinding, plan: &CheckPlan) -> Result<()> {
    let invalid = || {
        PmError::new(
            ErrorCode::InvalidSchema,
            "retained CI input binding differs from its exact plan or source proof",
        )
    };
    if binding.source.repository != plan.repository
        || binding.plan != plan.fingerprint
        || binding.inputs.len() != plan.invocations.len()
        || plan.checks.is_empty()
        || !plan.blockers.is_empty()
        || serde_json::to_vec(binding).map_err(|_| invalid())?.len()
            > crate::checks::MAX_CHECK_PLAN_BYTES
    {
        return Err(invalid());
    }
    for (manifest, invocation) in binding.inputs.iter().zip(&plan.invocations) {
        let absent: Vec<_> = invocation
            .inputs
            .entries
            .iter()
            .filter_map(|entry| match entry {
                InputEntry::Absent { path } => Some(path.clone()),
                _ => None,
            })
            .collect();
        if !invocation.inputs.complete
            || manifest.invocation != invocation.id
            || manifest.selection != invocation.input_selection
            || manifest.absent != absent
        {
            return Err(invalid());
        }
        let mut paths = BTreeSet::new();
        for entry in &manifest.entries {
            crate::commands::validation::relative(entry.path(), false)?;
            if !paths.insert(entry.path()) || crate::ci_evaluators::excluded(entry.path()) {
                return Err(invalid());
            }
        }
        let committed = CiEvaluatorManifest {
            check: manifest.invocation.clone(),
            selection: EvaluatorInputSelection {
                files: vec![],
                trees: manifest.selection.trees.clone(),
            },
            entries: manifest.entries.clone(),
            fingerprint: manifest.fingerprint.clone(),
        };
        if check_input_entries::local(&invocation.inputs)?
            != check_input_entries::committed(&committed)
        {
            return Err(invalid());
        }
        if manifest.fingerprint
            != crate::transactions::canonical_hash(&serde_json::json!({
                "domain":"workdeck.ci-invocation-inputs.v1", "invocation":manifest.invocation,
                "selection":manifest.selection, "entries":manifest.entries, "absent":manifest.absent,
            }))?
        {
            return Err(invalid());
        }
        // Git cannot contain an explicit root entry; root membership is derived
        // from the tree selection rather than caller-supplied directory records.
        if paths.contains(Path::new(".")) {
            return Err(invalid());
        }
    }
    if binding.fingerprint
        != crate::transactions::canonical_hash(&serde_json::json!({
            "domain":"workdeck.ci-check-input-binding.v1", "source":binding.source,
            "plan":binding.plan, "inputs":binding.inputs,
        }))?
    {
        return Err(invalid());
    }
    Ok(())
}
