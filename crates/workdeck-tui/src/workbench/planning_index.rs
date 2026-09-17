//! Bounded collection reader for the mounted native-planning authoring workspace.
use super::{PlanningWorkspace, Result};
use workdeck_pm::{
    ErrorCode, PlanningMembershipQuery, PmError, SnapshotKind, SourceSelector, projection::*,
};

pub(super) type MembershipWorker = crate::workbench::projection_worker::ProjectionWorker<
    MembershipJob,
    (ProjectionRowToken, Result<workdeck_pm::PlanningMembership>),
>;
#[derive(Debug)]
pub(crate) struct MembershipJob {
    token: ProjectionRowToken,
    query: PlanningMembershipQuery,
}

impl PlanningWorkspace {
    pub(super) fn refresh_index(&mut self) -> Result<()> {
        let query = ProjectionQuery::Planning {
            query: ProjectionPlanningQuery {
                kind: self.kind,
                query: String::new(),
                archive: self.issue_scope(),
                project: None,
                target: None,
            },
        };
        if self.index.is_none() {
            let root = self
                .repository()?
                .root()
                .parent()
                .ok_or_else(|| {
                    PmError::new(ErrorCode::InvalidInput, "Planning source has no worktree")
                })?
                .to_path_buf();
            self.index = Some(
                crate::workbench::indexed_workspace::IndexedWorkspace::new(
                    root.clone(),
                    SourceSelector::WorkingTree,
                    query.clone(),
                    ProjectionLimits::default(),
                )
                .map_err(|error| PmError::io(root, error))?,
            );
        }
        let index = self.index.as_mut().expect("planning reader");
        if index.query != query {
            index.set_query(query);
        }
        index.refresh(false);
        self.attempted = None;
        self.membership_requested = None;
        if let Some(worker) = &mut self.membership_worker {
            worker
                .invalidate()
                .map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn poll_index(&mut self) {
        self.poll_membership();
        let Some(index) = &mut self.index else {
            return;
        };
        index.poll();
        if !index.is_idle() || index.refreshing || index.querying || index.loading_page {
            return;
        }
        if let Some(error) = &index.error {
            self.error = Some(error.clone());
            return;
        }
        if let Some(error) = &index.worker_error {
            self.error = Some(PmError::new(ErrorCode::InvalidInput, error.clone()));
            return;
        }
        if index
            .handle
            .as_ref()
            .is_some_and(|handle| handle.total == 0)
        {
            self.pending_selected = None;
        }
        if let Some(id) = self.pending_selected.take() {
            let repository = index
                .handle
                .as_ref()
                .map(|handle| handle.view.source.repository.clone());
            if let Some(repository) = repository {
                index.locate(ProjectionRecordKey {
                    repository,
                    kind: planning_kind(self.kind),
                    id,
                });
                return;
            }
        }
        let token = index.selected_row().map(|row| row.token.clone());
        if token.is_none()
            && index
                .handle
                .as_ref()
                .is_some_and(|handle| handle.total == 0)
        {
            self.records.clear();
            self.selection.remove(&self.key());
            self.membership = None;
            self.attempted = None;
            self.membership_requested = None;
            return;
        }
        if token == self.attempted {
            return;
        }
        self.attempted = token.clone();
        if let Some(token) = token
            && let Err(error) = self.queue_membership(token)
        {
            self.error = Some(error);
        }
    }

    pub(super) fn select_index_record(&mut self) -> Result<()> {
        let token = self
            .index
            .as_ref()
            .and_then(|index| index.selected_row())
            .map(|row| row.token.clone())
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::NotFound,
                    "Select an available indexed planning record",
                )
            })?;
        let repository = self.repository()?;
        let record = repository.planning_from_projection(self.kind, &token)?;
        self.selection
            .insert(self.key(), record.metadata.id.clone());
        self.records = vec![record];
        self.attempted = Some(token.clone());
        self.queue_membership(token)
    }

    fn queue_membership(&mut self, token: ProjectionRowToken) -> Result<()> {
        if self.membership_requested.as_ref() != Some(&token) {
            if self.membership_worker.is_none() {
                let repository = self.repository()?.clone();
                self.membership_worker = Some(MembershipWorker::new(move |job: MembershipJob| {
                    let MembershipJob { token, query } = job;
                    let result = (|| {
                        let before = repository.planning_from_projection(query.kind, &token)?;
                        let membership = repository.planning_membership(&query)?;
                        let after = repository.planning_from_projection(query.kind, &token)?;
                        if membership.record != before || membership.record != after {
                            return Err(PmError::new(ErrorCode::StaleSource, "Selected indexed planning record changed while membership was read").at(&token.path));
                        }
                        Ok(membership)
                    })();
                    Ok((token, result))
                }).map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?);
            }
            let query = self.membership_query(&token.key.id);
            self.membership_worker
                .as_mut()
                .expect("membership reader")
                .request(
                    crate::workbench::projection_worker::ReadLane::Detail,
                    MembershipJob {
                        token: token.clone(),
                        query,
                    },
                )
                .map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?;
            self.membership_requested = Some(token);
            self.policy = None;
        }
        self.error = None;
        Ok(())
    }

    pub(crate) fn reader_idle(&self) -> bool {
        self.index.as_ref().is_none_or(|index| index.is_idle())
            && self
                .membership_worker
                .as_ref()
                .is_none_or(|worker| worker.is_idle())
    }

    fn poll_membership(&mut self) {
        let kind_key = self.key();
        let Some(worker) = &mut self.membership_worker else {
            return;
        };
        while let Some(completion) = worker.poll() {
            match completion.result {
                Ok((token, result)) if self.membership_requested.as_ref() == Some(&token) => {
                    self.membership_requested = None;
                    if self
                        .index
                        .as_ref()
                        .and_then(|index| index.selected_row())
                        .map(|row| &row.token)
                        != Some(&token)
                    {
                        continue;
                    }
                    match result {
                        Ok(membership) => {
                            self.selection
                                .insert(kind_key.clone(), membership.record.metadata.id.clone());
                            self.records = vec![membership.record.clone()];
                            self.membership = Some(membership);
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(error) => {
                    self.membership_requested = None;
                    self.error = Some(PmError::new(ErrorCode::Io, error));
                }
                _ => {}
            }
        }
    }

    pub(crate) fn take_index_shutdown(
        &mut self,
    ) -> (
        Option<crate::workbench::indexed_workspace::IndexedWorkspace>,
        Option<MembershipWorker>,
    ) {
        let mut index = self.index.take();
        let mut membership = self.membership_worker.take();
        if let Some(index) = &mut index {
            index.begin_shutdown();
        }
        if let Some(worker) = &mut membership {
            worker.begin_shutdown();
        }
        (index, membership)
    }
}

fn planning_kind(kind: workdeck_pm::PlanningKind) -> SnapshotKind {
    match kind {
        workdeck_pm::PlanningKind::Initiative => SnapshotKind::Initiative,
        workdeck_pm::PlanningKind::Project => SnapshotKind::Project,
        workdeck_pm::PlanningKind::Milestone => SnapshotKind::Milestone,
        workdeck_pm::PlanningKind::Cycle => SnapshotKind::Cycle,
        workdeck_pm::PlanningKind::Target => SnapshotKind::Target,
        workdeck_pm::PlanningKind::Label => SnapshotKind::Labels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    use workdeck_pm::{CreatePlanning, PlanningKind, Repository, RequestId};

    fn fixture() -> (tempfile::TempDir, Repository, PlanningWorkspace) {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        for kind in [PlanningKind::Project, PlanningKind::Cycle] {
            let mut input = CreatePlanning::new(format!("{kind:?} shared identity"));
            input.id = Some("shared".into());
            repository
                .create_planning(kind, &input, &RequestId::new())
                .unwrap();
        }
        let mut workspace = PlanningWorkspace::new_indexed(Some(repository.clone()));
        workspace.open(PlanningKind::Project);
        settle(&mut workspace);
        (directory, repository, workspace)
    }
    fn settle(workspace: &mut PlanningWorkspace) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            workspace.poll_index();
            if workspace.reader_idle() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "membership did not settle: {:?}",
                workspace.error
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn pending_membership_cannot_cross_kinds_with_the_same_record_id() {
        let (_directory, repository, mut workspace) = fixture();
        let token = workspace
            .index
            .as_ref()
            .unwrap()
            .selected_row()
            .unwrap()
            .token
            .clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut first = true;
        workspace.membership_worker = Some(
            MembershipWorker::new(move |job: MembershipJob| {
                let result = repository.planning_membership(&job.query);
                if first {
                    first = false;
                    started_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }
                Ok((job.token, result))
            })
            .unwrap(),
        );
        workspace.queue_membership(token).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        workspace.open(PlanningKind::Cycle);
        let deadline = Instant::now() + Duration::from_secs(2);
        while workspace
            .membership_requested
            .as_ref()
            .is_none_or(|token| token.key.kind != SnapshotKind::Cycle)
        {
            workspace.poll_index();
            assert!(
                Instant::now() < deadline,
                "kind switch blocked on previous membership"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        release_tx.send(()).unwrap();
        settle(&mut workspace);
        assert!(workspace.error.is_none(), "{:?}", workspace.error);
        assert_eq!(
            workspace.membership.as_ref().unwrap().record.kind,
            PlanningKind::Cycle
        );
        assert_eq!(
            workspace.membership.as_ref().unwrap().query.kind,
            PlanningKind::Cycle
        );
        assert_eq!(workspace.selected().unwrap().kind, PlanningKind::Cycle);
        assert_eq!(
            workspace.selection.get("Projects").map(String::as_str),
            Some("shared")
        );
        assert_eq!(
            workspace.selection.get("Cycles").map(String::as_str),
            Some("shared")
        );
    }

    #[test]
    fn current_membership_error_preserves_inspected_result_and_draft_preconditions() {
        let (_directory, _repository, mut workspace) = fixture();
        workspace.start_draft(true).unwrap();
        let previous = workspace.membership.as_ref().unwrap().fingerprint.clone();
        let source = workspace.active_draft().unwrap().expected.clone();
        let token = workspace
            .index
            .as_ref()
            .unwrap()
            .selected_row()
            .unwrap()
            .token
            .clone();
        workspace.membership_worker = Some(
            MembershipWorker::new(|job: MembershipJob| {
                Ok((
                    job.token,
                    Err(PmError::new(
                        ErrorCode::StaleSource,
                        "membership source changed",
                    )),
                ))
            })
            .unwrap(),
        );
        workspace.queue_membership(token).unwrap();
        settle(&mut workspace);
        assert_eq!(
            workspace.error.as_ref().unwrap().code,
            ErrorCode::StaleSource
        );
        assert_eq!(workspace.membership.as_ref().unwrap().fingerprint, previous);
        assert_eq!(workspace.active_draft().unwrap().expected, source);
    }

    #[test]
    fn planning_shutdown_joins_inflight_membership_after_transferring_ownership() {
        let (_directory, _repository, mut workspace) = fixture();
        let token = workspace
            .index
            .as_ref()
            .unwrap()
            .selected_row()
            .unwrap()
            .token
            .clone();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        workspace.membership_worker = Some(
            MembershipWorker::new(move |job: MembershipJob| {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok((
                    job.token,
                    Err(PmError::new(ErrorCode::StaleSource, "shutdown fixture")),
                ))
            })
            .unwrap(),
        );
        workspace.queue_membership(token).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let owners = workspace.take_index_shutdown();
        assert!(workspace.index.is_none() && workspace.membership_worker.is_none());
        let (done_tx, done_rx) = mpsc::channel();
        let join = std::thread::spawn(move || {
            drop(owners);
            done_tx.send(()).unwrap();
        });
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(25)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release_tx.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        join.join().unwrap();
    }
}
