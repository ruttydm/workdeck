//! Revision-bound foreground feedback. Producer trust is a separate admission.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiCheckRunRequest {
    pub input: CheckRunRequest,
    pub binding: CiCheckInputBinding,
    pub expected_binding: ContentHash,
}
impl Repository {
    pub fn run_ci_check_plan(
        &self,
        input: &CiCheckRunRequest,
        request: &RequestId,
        control: &RunControl,
    ) -> Result<RunOutcome> {
        self.run_ci_check_plan_with_faults(input, request, control, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn run_ci_check_plan_with_faults(
        &self,
        input: &CiCheckRunRequest,
        request: &RequestId,
        control: &RunControl,
        fault: impl FnMut(RunFaultPoint) -> Result<()>,
    ) -> Result<RunOutcome> {
        self.run_bound_check_plan_with_faults(
            &input.input,
            Some((&input.binding, &input.expected_binding)),
            request,
            control,
            fault,
        )
    }
}

/// A current local plan plus independently captured committed inputs. No process
/// has run, and neither this value nor its fingerprint is producer authority.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiPreparedCheck {
    pub plan: CheckPlan,
    pub binding: CiCheckInputBinding,
}

pub fn prepare_ci_check(
    worktree: &std::path::Path,
    revision: &CiRevision,
    request: &CheckPlanRequest,
) -> Result<CiPreparedCheck> {
    let validation = ci_validate(
        worktree,
        &CiValidateRequest {
            base: revision.clone(),
            head: revision.clone(),
        },
    )?;
    if !validation.valid {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "committed planning is invalid for CI check preparation",
        )
        .details(serde_json::json!({"validation":validation})));
    }
    let repository = Repository::open_source(&worktree.join(".workdeck"))?;
    let plan = repository.check_plan(request)?;
    require_evaluator_declarations(&plan)?;
    let binding = bind_ci_check_plan(worktree, &validation.head, &plan)?;
    binding.validate(&plan)?;
    Ok(CiPreparedCheck { plan, binding })
}

pub(crate) fn require_evaluator_declarations(plan: &CheckPlan) -> Result<()> {
    for check in &plan.checks {
        let definition = plan
            .definitions
            .checks
            .iter()
            .find(|record| record.definition.id == check.id)
            .ok_or_else(|| {
                PmError::new(ErrorCode::NotFound, "selected check definition is missing")
            })?;
        if definition.definition.evaluator_inputs.is_none() {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "CI checks require explicit evaluator_inputs; omitted is not empty",
            )
            .at(&definition.path));
        }
    }
    Ok(())
}
