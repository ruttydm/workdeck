use crate::{
    commands::{
        catalog,
        validation::{id, invalid, parameter_values},
    },
    execution::{
        input_fs,
        inputs::{self, CapturedInputs},
    },
    transactions::{Snapshot, canonical_hash},
    *,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub const MAX_CHECK_PLAN_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckPlanFaultPoint {
    BeforeInputValidation,
}
impl CheckPlan {
    pub fn validate(&self) -> Result<()> {
        validate_plan(self)
    }
}

impl Repository {
    pub fn check_plan(&self, request: &CheckPlanRequest) -> Result<CheckPlan> {
        self.plan_execution(
            &ExecutionPlanRequest::Checks {
                request: request.clone(),
            },
            &InputLimits::default(),
            &mut |_| Ok(()),
        )
    }
    pub fn command_plan(&self, request: &CommandPlanRequest) -> Result<CheckPlan> {
        self.plan_execution(
            &ExecutionPlanRequest::Command {
                request: request.clone(),
            },
            &InputLimits::default(),
            &mut |_| Ok(()),
        )
    }
    #[doc(hidden)]
    pub fn check_plan_with_limits(
        &self,
        request: &CheckPlanRequest,
        limits: &InputLimits,
        mut fault: impl FnMut(CheckPlanFaultPoint) -> Result<()>,
    ) -> Result<CheckPlan> {
        self.plan_execution(
            &ExecutionPlanRequest::Checks {
                request: request.clone(),
            },
            limits,
            &mut fault,
        )
    }
    #[doc(hidden)]
    pub fn command_plan_with_limits(
        &self,
        request: &CommandPlanRequest,
        limits: &InputLimits,
        mut fault: impl FnMut(CheckPlanFaultPoint) -> Result<()>,
    ) -> Result<CheckPlan> {
        self.plan_execution(
            &ExecutionPlanRequest::Command {
                request: request.clone(),
            },
            limits,
            &mut fault,
        )
    }
    fn plan_execution(
        &self,
        request: &ExecutionPlanRequest,
        limits: &InputLimits,
        fault: &mut dyn FnMut(CheckPlanFaultPoint) -> Result<()>,
    ) -> Result<CheckPlan> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let (plan, captured) = capture_plan(self.root(), snapshot, &config, request, limits)?;
            fault(CheckPlanFaultPoint::BeforeInputValidation)?;
            for capture in captured {
                capture.verify()?
            }
            Ok(plan)
        })
    }
    pub fn revalidate_check_plan(&self, plan: &CheckPlan) -> Result<()> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            prepare_plan(self.root(), snapshot, &config, plan).map(|_| ())
        })
    }
}
struct Selected<'a> {
    id: String,
    command: &'a CommandRecord,
    arguments: ArgumentValues,
    check: Option<&'a CheckRecord>,
}
fn selected<'a>(
    catalog: &'a CommandCatalogSnapshot,
    config: &Config,
    request: &ExecutionPlanRequest,
) -> Result<(Vec<Selected<'a>>, Vec<SelectionReason>, Vec<PlanDiagnostic>)> {
    let mut reasons = Vec::new();
    let mut blockers = Vec::new();
    let mut result = Vec::new();
    match request {
        ExecutionPlanRequest::Command { request } => {
            id(&request.command)?;
            let command = catalog
                .commands
                .iter()
                .find(|c| c.definition.id == request.command)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "command not found"))?;
            result.push(Selected {
                id: request.command.clone(),
                command,
                arguments: request.arguments.clone(),
                check: None,
            });
        }
        ExecutionPlanRequest::Checks { request } => {
            if request.checks.len() > 4096
                || request.profiles.len() > 4096
                || request.arguments.len() > 4096
                || request.changed_paths.len() > 4096
            {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "check selection exceeds supported bounds",
                ));
            }
            for path in &request.changed_paths {
                crate::commands::validation::relative(path, true)?
            }
            let mut ids = BTreeSet::new();
            for (values, reason) in [
                (&request.checks, "requested_check"),
                (&config.acceptance.required_checks, "required_check"),
            ] {
                for check in values {
                    id(check)?;
                    ids.insert(check.clone());
                    reasons.push(SelectionReason {
                        check: check.clone(),
                        reason_code: reason.into(),
                        source: None,
                    });
                }
            }
            for (values, reason) in [
                (&request.profiles, "requested_profile"),
                (&config.acceptance.required_profiles, "required_profile"),
            ] {
                for profile in values {
                    id(profile)?;
                    let profile = catalog
                        .profiles
                        .iter()
                        .find(|p| &p.definition.id == profile)
                        .ok_or_else(|| {
                            PmError::new(
                                ErrorCode::NotFound,
                                "required/requested check profile not found",
                            )
                        })?;
                    if profile.definition.archived {
                        blockers.push(PlanDiagnostic {
                            invocation: None,
                            reason_code: "profile_archived".into(),
                            message: format!("Profile {} is archived", profile.definition.id),
                        });
                    }
                    for check in &profile.definition.checks {
                        ids.insert(check.clone());
                        reasons.push(SelectionReason {
                            check: check.clone(),
                            reason_code: reason.into(),
                            source: Some(profile.definition.id.clone()),
                        });
                    }
                }
            }
            if ids.is_empty() {
                for check in catalog.checks.iter().filter(|c| !c.definition.archived) {
                    ids.insert(check.definition.id.clone());
                    reasons.push(SelectionReason {
                        check: check.definition.id.clone(),
                        reason_code: "all_active_checks".into(),
                        source: None,
                    });
                }
            }
            if request.arguments.keys().any(|id| !ids.contains(id)) {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "argument overrides name a check outside the selected set",
                ));
            }
            for check_id in ids {
                let check = catalog
                    .checks
                    .iter()
                    .find(|c| c.definition.id == check_id)
                    .ok_or_else(|| {
                        PmError::new(
                            ErrorCode::NotFound,
                            format!("required/requested check {check_id} not found"),
                        )
                    })?;
                let command = catalog
                    .commands
                    .iter()
                    .find(|c| c.definition.id == check.definition.command)
                    .ok_or_else(|| invalid("selected check command is missing"))?;
                if check.definition.archived {
                    blockers.push(PlanDiagnostic {
                        invocation: Some(check_id.clone()),
                        reason_code: "check_archived".into(),
                        message: format!("Check {check_id} is archived"),
                    });
                }
                if !request.changed_paths.is_empty() {
                    reasons.push(SelectionReason {
                        check: check_id.clone(),
                        reason_code: "impact_incomplete".into(),
                        source: None,
                    });
                }
                let mut arguments = check.definition.arguments.clone();
                if let Some(overrides) = request.arguments.get(&check_id) {
                    arguments.extend(overrides.clone());
                }
                result.push(Selected {
                    id: check_id,
                    command,
                    arguments,
                    check: Some(check),
                });
            }
        }
    }
    if result.len() > 128 {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "plan exceeds128 invocations; select a smaller explicit profile",
        ));
    }
    reasons.sort_by(|a, b| {
        (&a.check, &a.reason_code, &a.source).cmp(&(&b.check, &b.reason_code, &b.source))
    });
    reasons.dedup();
    if result.is_empty() {
        blockers.push(PlanDiagnostic {
            invocation: None,
            reason_code: "no_checks_selected".into(),
            message: "No checks are selected; there is no command to execute.".into(),
        });
    }
    Ok((result, reasons, blockers))
}
fn instantiate(
    command: &CommandDefinition,
    arguments: &ArgumentValues,
) -> Result<(String, Vec<PlannedArgument>)> {
    if arguments
        .keys()
        .any(|name| !command.parameters.contains_key(name))
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "unknown command argument",
        ));
    }
    let mut values = BTreeMap::new();
    for (name, param) in &command.parameters {
        let value = arguments
            .get(name)
            .or(param.default.as_ref())
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::InvalidInput,
                    format!("required argument {name} is missing"),
                )
            })?;
        values.insert(name, parameter_values(&param.value_type, value)?);
    }
    let (tool, tokens, mut result) = match &command.recipe {
        CommandRecipe::Argv { argv } => {
            let Some(ArgumentToken::Literal { value }) = argv.first() else {
                return Err(invalid("argv[0] is not a declared tool"));
            };
            (value.clone(), &argv[1..], Vec::new())
        }
        CommandRecipe::Shell {
            interpreter,
            script,
            args,
        } => (
            interpreter.clone(),
            args.as_slice(),
            vec![
                PlannedArgument::Literal { value: "-c".into() },
                PlannedArgument::Literal {
                    value: script.clone(),
                },
                PlannedArgument::Literal {
                    value: format!("workdeck:{}", command.id),
                },
            ],
        ),
    };
    for token in tokens {
        match token {
            ArgumentToken::Literal { value } => result.push(PlannedArgument::Literal {
                value: value.clone(),
            }),
            ArgumentToken::Parameter { name } | ArgumentToken::Parameters { name } => {
                let values = values
                    .get(name)
                    .ok_or_else(|| invalid("argument references missing parameter"))?;
                result.extend(values.iter().map(|value| PlannedArgument::Literal {
                    value: value.clone(),
                }));
            }
            ArgumentToken::Artifact { id } => result.push(PlannedArgument::ArtifactPath {
                artifact: id.clone(),
            }),
        }
    }
    let bytes = serde_json::to_vec(&result).map_err(|e| invalid(e.to_string()))?;
    if result.len() > 256 || bytes.len() > 256 * 1024 {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "resolved argv exceeds256 elements or256KiB",
        ));
    }
    Ok((tool, result))
}
fn invocation_hash(value: &PlannedInvocation) -> Result<ContentHash> {
    let mut value = serde_json::to_value(value).map_err(|e| invalid(e.to_string()))?;
    value.as_object_mut().unwrap().remove("fingerprint");
    canonical_hash(&value)
}
fn invocation(selected: &Selected<'_>, inputs: InputManifest) -> Result<PlannedInvocation> {
    let definition = &selected.command.definition;
    let (tool, args) = instantiate(definition, &selected.arguments)?;
    let mut invocation = PlannedInvocation {
        id: selected.id.clone(),
        command: definition.id.clone(),
        definition: selected.command.content.clone(),
        tool,
        args,
        cwd: definition.cwd.clone(),
        environment: definition.environment.clone(),
        tools: definition.tools.clone(),
        input_selection: definition.inputs.clone(),
        inputs,
        bounds: definition.bounds.clone(),
        artifacts: definition.artifacts.clone(),
        effects: definition.effects.clone(),
        fingerprint: ContentHash::of(b""),
    };
    invocation.fingerprint = invocation_hash(&invocation)?;
    Ok(invocation)
}
fn invocation_blockers(
    invocation: &PlannedInvocation,
    command: &CommandDefinition,
) -> Vec<PlanDiagnostic> {
    let mut result = Vec::new();
    let mut add = |code: &str, message: String| {
        result.push(PlanDiagnostic {
            invocation: Some(invocation.id.clone()),
            reason_code: code.into(),
            message,
        })
    };
    if command.archived {
        add(
            "command_archived",
            format!("Command {} is archived", command.id),
        );
    }
    for tool in &invocation.inputs.tools {
        if tool.content.is_none() {
            add(
                "tool_missing",
                format!("Declared tool {} is unavailable", tool.name),
            );
        }
    }
    for (name, value) in &command.environment {
        if matches!(value, EnvironmentValue::Inherit { required: true, .. })
            && !invocation
                .inputs
                .environment
                .iter()
                .any(|p| &p.name == name && p.present)
        {
            add(
                "environment_missing",
                format!("Required environment name {name} is unset"),
            );
        }
    }
    result
}
fn plan_hash(value: &CheckPlan) -> Result<ContentHash> {
    let mut value = serde_json::to_value(value).map_err(|e| invalid(e.to_string()))?;
    value.as_object_mut().unwrap().remove("fingerprint");
    canonical_hash(&value)
}
fn subject(repository: &RepositoryId, invocations: &[PlannedInvocation]) -> Result<ExactSubject> {
    Ok(ExactSubject {
        repository: repository.clone(),
        kind: ExactSubjectKind::Source,
        content: canonical_hash(&serde_json::json!(
            invocations
                .iter()
                .map(|i| (&i.id, &i.inputs.fingerprint))
                .collect::<Vec<_>>()
        ))?,
    })
}
fn capture_plan(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    request: &ExecutionPlanRequest,
    limits: &InputLimits,
) -> Result<(CheckPlan, Vec<CapturedInputs>)> {
    inputs::worktree(root)?;
    let catalog = catalog::capture(snapshot, config)?;
    let (selected, selection, mut blockers) = selected(&catalog, config, request)?;
    let mut captures = Vec::new();
    let mut invocations = Vec::new();
    let mut checks = Vec::new();
    for selected in selected {
        let mut input = inputs::capture(
            root,
            &selected.command.definition.inputs,
            &selected.command.definition.environment,
            &selected.command.definition.tools,
            limits,
        )?;
        input.bind_cwd(&selected.command.definition.cwd)?;
        let invocation = invocation(&selected, input.manifest.clone())?;
        blockers.extend(invocation_blockers(
            &invocation,
            &selected.command.definition,
        ));
        if let Some(check) = selected.check {
            checks.push(PlannedCheck {
                id: check.definition.id.clone(),
                definition: check.content.clone(),
                invocation: invocation.id.clone(),
                expectation: check.definition.expectation.clone(),
            });
        }
        captures.push(input);
        invocations.push(invocation);
    }
    let issue = match request {
        ExecutionPlanRequest::Checks { request } => request.issue.as_deref(),
        _ => None,
    }
    .map(|reference| {
        let issue = crate::issues::resolve_issue(root, snapshot, config, reference)?;
        let requirements =
            crate::context::requirement_fingerprint(root, snapshot, config, &issue.metadata.id)?;
        Ok::<_, PmError>(PlanIssue {
            id: issue.metadata.id,
            source: issue.source,
            requirements,
        })
    })
    .transpose()?;
    let config_bytes = snapshot
        .read(Path::new("config.yml"))?
        .ok_or_else(|| invalid("config missing"))?;
    let mut plan = CheckPlan {
        schema: SchemaVersion::CURRENT,
        repository: config.repository.clone(),
        basis: VerificationBasis::LocalFeedback,
        request: request.clone(),
        issue,
        config: ContentHash::of(&config_bytes),
        configuration: config.clone(),
        config_document: String::from_utf8(config_bytes)
            .map_err(|_| invalid("config is not UTF-8"))?,
        definitions: catalog,
        subject: subject(&config.repository, &invocations)?,
        invocations,
        checks,
        selection,
        blockers,
        fingerprint: ContentHash::of(b""),
    };
    if !plan.invocations.is_empty()
        && let Err(error) = crate::execution::validate_run_bounds(&plan)
    {
        plan.blockers.push(PlanDiagnostic {
            invocation: None,
            reason_code: "execution_limits".into(),
            message: error.message,
        });
    }
    plan.fingerprint = plan_hash(&plan)?;
    if serde_json::to_vec(&plan)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        > MAX_CHECK_PLAN_BYTES
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "complete check plan exceeds4MiB; select a smaller catalog/profile",
        ));
    }
    for capture in &captures {
        capture.verify()?
    }
    validate_plan(&plan)?;
    Ok((plan, captures))
}
/// Validate a reviewed plan inside the caller's replay-first reservation snapshot.
pub(crate) fn prepare_plan(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    plan: &CheckPlan,
) -> Result<Vec<CapturedInputs>> {
    validate_plan(plan)?;
    let limits = plan
        .invocations
        .first()
        .map(|i| i.inputs.limits.clone())
        .unwrap_or_default();
    let (current, captured) = capture_plan(root, snapshot, config, &plan.request, &limits)?;
    if current != *plan {
        return Err(input_fs::stale()
            .hint("Inspect a fresh check plan and explicitly run that new fingerprint."));
    }
    if !current.blockers.is_empty() {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "check plan has unresolved prerequisites or archived definitions",
        )
        .details(serde_json::json!({"blockers":current.blockers})));
    }
    Ok(captured)
}
/// Structural historical proof only: never reads current source or promotes results.
pub(crate) fn validate_plan(plan: &CheckPlan) -> Result<()> {
    if serde_json::to_vec(plan)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        > MAX_CHECK_PLAN_BYTES
    {
        return Err(invalid("plan exceeds4MiB"));
    }
    let config =
        crate::repository::parse_config(Path::new("config.yml"), plan.config_document.as_bytes())?;
    if config != plan.configuration
        || config.repository != plan.repository
        || ContentHash::of(plan.config_document.as_bytes()) != plan.config
        || plan.definitions.repository != plan.repository
    {
        return Err(invalid("plan configuration/repository proof mismatch"));
    }
    catalog::validate_snapshot(&plan.definitions)?;
    let (selected, reasons, mut blockers) = selected(&plan.definitions, &config, &plan.request)?;
    if selected.len() != plan.invocations.len() || reasons != plan.selection {
        return Err(invalid(
            "plan selection differs from its request and captured policy",
        ));
    }
    let mut checks = Vec::new();
    let mut limits = None;
    for (selected, current) in selected.iter().zip(&plan.invocations) {
        inputs::validate_manifest(&current.inputs)?;
        inputs::validate_bindings(
            &current.inputs,
            &selected.command.definition.environment,
            &selected.command.definition.tools,
        )?;
        if current.inputs.selection != selected.command.definition.inputs
            || limits.as_ref().is_some_and(|l| l != &current.inputs.limits)
        {
            return Err(invalid(
                "plan input scope/limits differ from its definition",
            ));
        }
        limits = Some(current.inputs.limits.clone());
        if invocation(selected, current.inputs.clone())? != *current {
            return Err(invalid(
                "planned invocation differs from its exact recipe/arguments",
            ));
        }
        blockers.extend(invocation_blockers(current, &selected.command.definition));
        if let Some(check) = selected.check {
            checks.push(PlannedCheck {
                id: check.definition.id.clone(),
                definition: check.content.clone(),
                invocation: current.id.clone(),
                expectation: check.definition.expectation.clone(),
            });
        }
    }
    if !plan.invocations.is_empty()
        && let Err(error) = crate::execution::validate_run_bounds(plan)
    {
        blockers.push(PlanDiagnostic {
            invocation: None,
            reason_code: "execution_limits".into(),
            message: error.message,
        });
    }
    if checks != plan.checks
        || blockers != plan.blockers
        || subject(&plan.repository, &plan.invocations)? != plan.subject
        || plan_hash(plan)? != plan.fingerprint
    {
        return Err(invalid(
            "plan check/subject/blocker/fingerprint proof mismatch",
        ));
    }
    let has_issue =
        matches!(&plan.request,ExecutionPlanRequest::Checks{request}if request.issue.is_some());
    if has_issue != plan.issue.is_some() {
        return Err(invalid("plan issue basis does not match its request"));
    }
    Ok(())
}
