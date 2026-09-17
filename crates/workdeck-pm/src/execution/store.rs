use super::{records::*, *};
use crate::{
    transactions::{FileChange, MutationReceipt, PreparedOperation, Snapshot},
    *,
};
use std::{path::Path, sync::atomic::Ordering};

struct Cleanup<'a>(&'a RunControl);
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        self.0.cleanup.store(
            !self.0.cleanup_failed.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
    }
}

impl Repository {
    pub fn run_check_plan(
        &self,
        input: &CheckRunRequest,
        request: &RequestId,
        control: &RunControl,
    ) -> Result<RunOutcome> {
        self.run_check_plan_with_faults(input, request, control, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn run_check_plan_with_faults(
        &self,
        input: &CheckRunRequest,
        request: &RequestId,
        control: &RunControl,
        fault: impl FnMut(RunFaultPoint) -> Result<()>,
    ) -> Result<RunOutcome> {
        self.run_bound_check_plan_with_faults(input, None, request, control, fault)
    }
    pub(crate) fn run_bound_check_plan_with_faults(
        &self,
        input: &CheckRunRequest,
        revision: Option<(&CiCheckInputBinding, &ContentHash)>,
        request: &RequestId,
        control: &RunControl,
        mut fault: impl FnMut(RunFaultPoint) -> Result<()>,
    ) -> Result<RunOutcome> {
        control.cleanup.store(false, Ordering::SeqCst);
        control.cleanup_failed.store(false, Ordering::SeqCst);
        let _cleanup = Cleanup(control);
        #[cfg(not(unix))]
        {
            let _ = (input, request, &mut fault);
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "foreground execution has not been qualified on this platform",
            ));
        }
        let revision = revision
            .map(|(binding, expected)| {
                if expected != &binding.fingerprint {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "expected CI input binding differs from supplied binding",
                    ));
                }
                binding.validate(&input.plan)?;
                crate::ci_execution::require_evaluator_declarations(&input.plan)?;
                Ok(binding)
            })
            .transpose()?;
        let mut bindings = None;
        let receipt = self.store()?.transact(
            request,
            RESERVE,
            &reservation_input(input, revision)?,
            |snapshot| {
                let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                validate_run_bounds(&input.plan)?;
                if input.expected_plan != input.plan.fingerprint {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "expected plan fingerprint differs from supplied plan",
                    ));
                }
                crate::organization::validate_actor(snapshot, &config.repository, &input.actor)?;
                let captured =
                    crate::checks::prepare_plan(self.root(), snapshot, &config, &input.plan)?;
                if !input.plan.blockers.is_empty() {
                    return Err(PmError::new(
                        ErrorCode::PolicyBlocked,
                        "execution plan has unresolved blockers",
                    )
                    .details(serde_json::json!({"blockers":input.plan.blockers})));
                }
                let worktree = inputs::worktree(self.root())?;
                if captured
                    .iter()
                    .any(|capture| capture.worktree() != worktree)
                {
                    return Err(invalid("invocations are bound to different worktree roots"));
                }
                if let Some(revision) = revision {
                    revision.validate(&input.plan)?;
                    if crate::sources::bind_check_inputs(&worktree, &revision.source, &input.plan)?
                        != revision.inputs
                    {
                        return Err(PmError::new(
                            ErrorCode::StaleSource,
                            "CI input binding changed before reservation",
                        ));
                    }
                    for capture in &captured {
                        capture.verify()?;
                    }
                }
                let mut intent = RunIntent {
                    schema: SchemaVersion::CURRENT,
                    repository: config.repository,
                    id: LocalRunId::new(),
                    request_id: request.clone(),
                    recorded_at: chrono::Utc::now(),
                    worktree,
                    input: input.clone(),
                    revision: revision.cloned(),
                    invocations: Vec::new(),
                };
                if captured.len() != input.plan.invocations.len() {
                    return Err(invalid("prepared invocation membership differs from plan"));
                }
                for (index, capture) in captured.iter().enumerate() {
                    let planned = &input.plan.invocations[index];
                    let executable = capture.tool(&planned.tool)?;
                    let mut argv = vec![
                        executable
                            .to_str()
                            .ok_or_else(|| invalid("resolved executable path must be UTF8"))?
                            .into(),
                    ];
                    argv.extend(resolved_arguments(&intent, index)?);
                    intent.invocations.push(ResolvedInvocation {
                        argv,
                        cwd: intent.worktree.join(&planned.cwd),
                        fingerprint: planned.fingerprint.clone(),
                    });
                }
                let run = record(intent)?;
                validate_intent(&run)?;
                bindings = Some(captured);
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: run.path.clone(),
                        expected: None,
                        content: Some(run.document.as_bytes().to_vec()),
                    }],
                    result: json_value(&run)?,
                })
            },
        )?;
        validate_receipt(&receipt)?;
        let run: RunRecord =
            serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
        let Some(bindings) = bindings else {
            let mut outcome = self.recover_run(&run.intent.id)?;
            outcome.replayed = true;
            return Ok(outcome);
        };
        let mut committed = vec![receipt.clone()];
        let attempt = (|| {
            fault(RunFaultPoint::AfterIntent)?;
            let local_root = local::prepare(self.root(), &run.intent.id)?;
            let _run_lock = local::RunLock::acquire(self.root(), &run.intent.id)?;
            // This create-only local marker is retained even if the caller loses
            // its acknowledgement before spawn. Replay never interprets absence
            // of a terminal result as permission to execute again.
            local::publish(
                self.root(),
                &local_root.join("invocation.json"),
                &serde_json::to_vec(&serde_json::json!({"run":run.intent.id,"intent":run.content}))
                    .map_err(|e| invalid(e.to_string()))?,
                None,
            )?;
            let started = chrono::Utc::now();
            let mut invocations = Vec::new();
            let mut checks = Vec::new();
            let mut stopped = false;
            for (index, capture) in bindings.iter().enumerate() {
                let planned = &run.intent.input.plan.invocations[index];
                let directory = local_root.join(format!("invocation-{index}"));
                local::directory(self.root(), &directory)?;
                local::directory(self.root(), &directory.join("artifacts"))?;
                let mut source_current = !stopped;
                if source_current {
                    fault(RunFaultPoint::BeforeSpawn)?;
                    source_current = capture.verify().is_ok()
                        && self
                            .store()?
                            .with_snapshot(|snapshot| {
                                let (current, _) = load_run(self.root(), snapshot, &run.intent.id)?;
                                if current != run {
                                    return Err(PmError::new(
                                        ErrorCode::StaleSource,
                                        "run intent changed before spawn",
                                    ));
                                }
                                let config =
                                    crate::repository::config_from_snapshot(self.root(), snapshot)?;
                                crate::checks::prepare_plan(
                                    self.root(),
                                    snapshot,
                                    &config,
                                    &run.intent.input.plan,
                                )
                                .map(|_| ())
                            })
                            .is_ok();
                }
                let process = if source_current && !control.cancellation_requested() {
                    process::execute(
                        self.root(),
                        &directory,
                        &run.intent.invocations[index],
                        &capture.environment(),
                        &planned.bounds,
                        control,
                        &mut fault,
                    )?
                } else {
                    stopped = true;
                    not_run(
                        self.root(),
                        &directory,
                        if control.cancellation_requested() {
                            "canceled_before_spawn"
                        } else {
                            "plan_changed_before_spawn"
                        },
                    )?
                };
                fault(RunFaultPoint::AfterProcess)?;
                let unchanged = source_current
                    && capture.verify().is_ok()
                    && self
                        .store()?
                        .with_snapshot(|snapshot| {
                            let config =
                                crate::repository::config_from_snapshot(self.root(), snapshot)?;
                            crate::checks::prepare_plan(
                                self.root(),
                                snapshot,
                                &config,
                                &run.intent.input.plan,
                            )
                            .map(|_| ())
                        })
                        .is_ok();
                if !unchanged || control.cancellation_requested() || !process.cleanup_complete {
                    stopped = true;
                }
                let (artifacts, bytes) = assessment::artifacts(self.root(), &run.intent, index);
                let check = run
                    .intent
                    .input
                    .plan
                    .checks
                    .iter()
                    .find(|check| check.invocation == planned.id);
                let codes = check
                    .map(|check| assessment::allowed(&check.expectation))
                    .unwrap_or(&[0]);
                let state =
                    assessment::invocation_state(planned, &process, &artifacts, unchanged, codes);
                let mut reasons = process.reason_codes.clone();
                for (spec, artifact) in planned.artifacts.iter().zip(&artifacts) {
                    if spec.required && artifact.availability != ArtifactAvailability::Present {
                        reasons.push(
                            match artifact.availability {
                                ArtifactAvailability::Missing => "required_artifact_missing",
                                ArtifactAvailability::TooLarge => "required_artifact_too_large",
                                ArtifactAvailability::Unsafe => "required_artifact_unsafe",
                                _ => "required_artifact_changed",
                            }
                            .into(),
                        );
                    }
                }
                if !unchanged {
                    reasons.push("inputs_changed_or_not_run".into());
                }
                let outcome = InvocationOutcome {
                    index,
                    invocation: planned.fingerprint.clone(),
                    process,
                    artifacts,
                    inputs_unchanged: unchanged,
                    state,
                    reason_codes: reasons,
                };
                if let Some(check) = check {
                    let artifact = assessment::report_artifact(&check.expectation)
                        .and_then(|id| bytes.get(id))
                        .map(Vec::as_slice);
                    let report = crate::assess_check_report(&check.expectation, artifact);
                    checks.push(CheckOutcome {
                        check: CheckRef {
                            id: check.id.clone(),
                            definition: check.definition.clone(),
                        },
                        invocation: index,
                        state: assessment::check_state(state, report.state),
                        reason_codes: report.reason_codes.clone(),
                        report,
                    });
                }
                invocations.push(outcome);
            }
            let state = assessment::aggregate(
                invocations
                    .iter()
                    .map(|i| i.state)
                    .chain(checks.iter().map(|c| c.state)),
            );
            let result = result_record(RunResult {
                schema: SchemaVersion::CURRENT,
                repository: run.intent.repository.clone(),
                id: run.intent.id.clone(),
                request_id: request.clone(),
                intent_content: run.content.clone(),
                started_at: started,
                finished_at: chrono::Utc::now(),
                invocations,
                checks,
                state,
                basis: "local_feedback".into(),
            })?;
            validate_pair(&run, &result)?;
            fault(RunFaultPoint::BeforeResultJournal)?;
            local::publish(
                self.root(),
                &local_root.join("terminal.yml"),
                result.document.as_bytes(),
                None,
            )?;
            fault(RunFaultPoint::AfterResultJournal)?;
            committed.push(self.publish_run_result_with_faults(&run, &result, &mut fault)?);
            fault(RunFaultPoint::AfterResultPublication)?;
            self.check_status(&run.intent.id)
        })();
        attempt.map_err(|error|error.details(serde_json::json!({"mutation_committed":true,"run":run.intent.id,"receipt":receipt,"receipts":committed,"cleanup_complete":!control.cleanup_failed.load(Ordering::SeqCst)})))
    }
    fn publish_run_result(
        &self,
        run: &RunRecord,
        result: &RunResultRecord,
    ) -> Result<MutationReceipt> {
        self.publish_run_result_with_faults(run, result, &mut |_| Ok(()))
    }
    fn publish_run_result_with_faults(
        &self,
        run: &RunRecord,
        result: &RunResultRecord,
        fault: &mut impl FnMut(RunFaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        validate_pair(run, result)?;
        let request = finish_request(&run.intent.id)?;
        let receipt = self.store()?.transact_with_faults(
            &request,
            FINISH,
            &finish_input(run, result),
            |snapshot| {
                let (current, existing) = load_run(self.root(), snapshot, &run.intent.id)?;
                if &current != run {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "retained run intent changed before result publication",
                    ));
                }
                if existing.is_some() {
                    return Err(PmError::new(
                        ErrorCode::Conflict,
                        "run result exists without matching publication receipt",
                    ));
                }
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: result.path.clone(),
                        expected: None,
                        content: Some(result.document.as_bytes().to_vec()),
                    }],
                    result: json_value(&RunPublication {
                        intent: run.clone(),
                        result: result.clone(),
                    })?,
                })
            },
            |point| {
                if point == crate::transactions::FaultPoint::AfterChange(0) {
                    fault(RunFaultPoint::AfterResultFile)?;
                }
                Ok(())
            },
        )?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
    pub fn recover_run(&self, id: &LocalRunId) -> Result<RunOutcome> {
        let current = self.check_status(id)?;
        if current.results.is_some() || current.state == RunState::Running {
            return Ok(current);
        }
        let root = local::root(self.root(), id);
        let Some(bytes) = local::read(
            self.root(),
            &root.join("terminal.yml"),
            MAX_RUN_RECORD_BYTES,
        )?
        else {
            return Ok(current);
        };
        let _lock = match local::RunLock::acquire(self.root(), id) {
            Ok(lock) => lock,
            Err(error) if error.code == ErrorCode::Locked => return self.check_status(id),
            Err(error) => return Err(error),
        };
        if local::read(
            self.root(),
            &root.join("terminal.yml"),
            MAX_RUN_RECORD_BYTES,
        )?
        .as_deref()
            != Some(bytes.as_slice())
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "local terminal result changed before recovery",
            ));
        }
        let RunDocument::Result(result) = parse(&result_path(id), &bytes, self.identity())? else {
            unreachable!()
        };
        validate_pair(&current.run, &result)?;
        self.publish_run_result(&current.run, &result)?;
        self.check_status(id)
    }
    pub fn check_status(&self, id: &LocalRunId) -> Result<RunOutcome> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            super::status_from_snapshot(self.root(), snapshot, &config, id)
        })
    }
    pub fn check_results(&self, query: &RunQuery) -> Result<Vec<RunOutcome>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            super::results_from_snapshot(self.root(), snapshot, &config, query)
        })
    }
}

fn not_run(root: &Path, directory: &Path, reason: &str) -> Result<ProcessObservation> {
    let log = |name: &str| -> Result<LogDescriptor> {
        let path = directory.join(name);
        local::create_file(root, &path)?
            .sync_all()
            .map_err(|e| PmError::io(&path, e))?;
        Ok(LogDescriptor {
            path: path.strip_prefix(root).expect("checked local root").into(),
            content: ContentHash::of(b""),
            retained_bytes: 0,
            observed_bytes: 0,
            truncated: false,
        })
    };
    Ok(ProcessObservation {
        started_at: None,
        finished_at: chrono::Utc::now(),
        elapsed_millis: 0,
        termination: ProcessTermination::NotRun,
        exit_code: None,
        signal: None,
        cleanup_complete: true,
        stdout: log("stdout.log")?,
        stderr: log("stderr.log")?,
        reason_codes: vec![reason.into()],
    })
}

pub(crate) fn status_from_snapshot(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &LocalRunId,
) -> Result<RunOutcome> {
    let (run, result) = load_run(root, snapshot, id)?;
    status_loaded(root, snapshot, config, run, result)
}
fn status_loaded(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    run: RunRecord,
    result: Option<RunResultRecord>,
) -> Result<RunOutcome> {
    let catalog = receipt_catalog(snapshot)?;
    status_with_catalog(root, snapshot, config, run, result, &catalog)
}
fn status_with_catalog(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    run: RunRecord,
    result: Option<RunResultRecord>,
    catalog: &ReceiptCatalog,
) -> Result<RunOutcome> {
    let mut receipts = receipts_for_record(catalog, &run)?;
    let reserve = receipts
        .get(RESERVE)
        .ok_or_else(|| invalid("run intent lacks its original reservation receipt"))?;
    if serde_json::from_value::<RunRecord>(reserve.result.clone())
        .map_err(|e| invalid(e.to_string()))?
        != run
    {
        return Err(invalid(
            "run intent differs from retained reservation proof",
        ));
    }
    if let Some(result) = &result {
        let receipt = receipts
            .get(FINISH)
            .ok_or_else(|| invalid("run result lacks its publication receipt"))?;
        let publication: RunPublication =
            serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
        if publication.intent != run || &publication.result != result {
            return Err(invalid(
                "run result differs from retained publication proof",
            ));
        }
    } else if receipts.contains_key(FINISH) {
        return Err(invalid("published run result is missing"));
    }
    let mut assessment = LocalResultAssessment {
        run: run.intent.id.clone(),
        state: RunState::Unknown,
        historical_state: result
            .as_ref()
            .map(|r| r.result.state)
            .unwrap_or(RunState::Unknown),
        basis: "local_feedback".into(),
        reason_codes: Vec::new(),
        artifacts: Vec::new(),
    };
    let local_root = local::root(root, &run.intent.id);
    if let Some(result) = &result {
        let mut current = true;
        match local::read(root, &local_root.join("terminal.yml"), MAX_RUN_RECORD_BYTES) {
            Ok(Some(bytes)) if bytes == result.document.as_bytes() => (),
            _ => {
                current = false;
                assessment
                    .reason_codes
                    .push("local_result_proof_missing_or_changed".into());
            }
        }
        for (index, invocation) in result.result.invocations.iter().enumerate() {
            for log in [&invocation.process.stdout, &invocation.process.stderr] {
                match local::read(root, &root.join(&log.path), MAX_LOCAL_LOG_BYTES as usize) {
                    Ok(Some(bytes))
                        if bytes.len() as u64 == log.retained_bytes
                            && ContentHash::of(&bytes) == log.content => {}
                    _ => {
                        current = false;
                        assessment
                            .reason_codes
                            .push("local_log_missing_or_changed".into());
                    }
                }
            }
            let (mut artifacts, _) = assessment::artifacts(root, &run.intent, index);
            for (actual, expected) in artifacts.iter_mut().zip(&invocation.artifacts) {
                if actual.availability == ArtifactAvailability::Present
                    && actual.content != expected.content
                {
                    actual.availability = ArtifactAvailability::Changed;
                }
                if actual != expected {
                    current = false;
                    assessment
                        .reason_codes
                        .push("local_artifact_missing_or_changed".into());
                }
            }
            assessment.artifacts.extend(artifacts);
        }
        let fresh =
            crate::checks::prepare_plan(root, snapshot, config, &run.intent.input.plan).is_ok();
        assessment.state = if !current {
            RunState::Unknown
        } else if !fresh {
            assessment
                .reason_codes
                .push("planned_inputs_or_definitions_changed".into());
            RunState::Stale
        } else {
            result.result.state
        };
    } else if local::locked(root, &run.intent.id)? {
        assessment.state = RunState::Running;
        assessment
            .reason_codes
            .push("foreground_owner_active".into());
    } else {
        assessment
            .reason_codes
            .push("execution_outcome_ambiguous_no_automatic_respawn".into());
    }
    if let Some(result) = &result {
        for invocation in &result.result.invocations {
            if invocation.state != RunState::Passed {
                assessment
                    .reason_codes
                    .extend(invocation.reason_codes.iter().cloned());
            }
        }
        for check in &result.result.checks {
            if check.state != RunState::Passed {
                assessment
                    .reason_codes
                    .extend(check.reason_codes.iter().cloned());
            }
        }
    }
    assessment.reason_codes.sort();
    assessment.reason_codes.dedup();
    Ok(RunOutcome {
        state: assessment.state,
        run,
        results: result,
        assessment,
        receipts: [RESERVE, FINISH]
            .into_iter()
            .filter_map(|operation| receipts.remove(operation))
            .collect(),
        replayed: false,
    })
}
pub(crate) fn results_from_snapshot(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    query: &RunQuery,
) -> Result<Vec<RunOutcome>> {
    let limit = query.limit.unwrap_or(4096);
    if query.limit.is_some_and(|limit| limit == 0 || limit > 100) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "run query limit must be 1..100",
        ));
    }
    let mut records = load_runs(root, snapshot)?;
    records.sort_by(|a, b| {
        b.0.intent
            .recorded_at
            .cmp(&a.0.intent.recorded_at)
            .then(b.0.intent.id.cmp(&a.0.intent.id))
    });
    let mut result = Vec::new();
    let catalog = receipt_catalog(snapshot)?;
    for (run, record) in records {
        if query.issue.as_ref().is_some_and(|id| {
            run.intent
                .input
                .plan
                .issue
                .as_ref()
                .is_none_or(|issue| &issue.id != id)
        }) {
            continue;
        }
        let outcome = status_with_catalog(root, snapshot, config, run, record, &catalog)?;
        if query.state.is_none_or(|state| state == outcome.state) {
            result.push(outcome);
            if result.len() == limit {
                break;
            }
        }
    }
    Ok(result)
}
pub(crate) fn results_for_context(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueId,
    limit: usize,
) -> Result<(Vec<RunOutcome>, usize, ContentHash)> {
    if limit == 0 || limit > 100 {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "context run limit must be 1..100",
        ));
    }
    let mut records = load_runs(root, snapshot)?
        .into_iter()
        .filter(|(run, _)| {
            run.intent
                .input
                .plan
                .issue
                .as_ref()
                .is_some_and(|record| &record.id == issue)
        })
        .collect::<Vec<_>>();
    records.sort_by(|a, b| {
        b.0.intent
            .recorded_at
            .cmp(&a.0.intent.recorded_at)
            .then(b.0.intent.id.cmp(&a.0.intent.id))
    });
    let total = records.len();
    let membership=records.iter().map(|(run,result)|serde_json::json!({"intent":{"path":run.path,"content":run.content},"result":result.as_ref().map(|result|serde_json::json!({"path":result.path,"content":result.content}))})).collect::<Vec<_>>();
    let catalog = receipt_catalog(snapshot)?;
    let outcomes = records
        .into_iter()
        .take(limit)
        .map(|(run, result)| status_with_catalog(root, snapshot, config, run, result, &catalog))
        .collect::<Result<Vec<_>>>()?;
    let fingerprint = crate::transactions::canonical_hash(
        &serde_json::json!({"membership":membership,"assessments":outcomes.iter().map(|outcome|&outcome.assessment).collect::<Vec<_>>()}),
    )?;
    Ok((outcomes, total, fingerprint))
}
