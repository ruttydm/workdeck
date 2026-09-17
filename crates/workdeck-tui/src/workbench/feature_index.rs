//! Bounded collection reader for the mounted native-feature authoring workspace.
use super::{FeatureWorkspace, Result};
use workdeck_pm::{
    ArchiveFilter, ErrorCode, FeatureCoverageQuery, PmError, SnapshotKind, SourceSelector,
    projection::*,
};

pub(super) type CoverageWorker = crate::workbench::projection_worker::ProjectionWorker<
    ProjectionRowToken,
    (ProjectionRowToken, Result<workdeck_pm::FeatureCoverage>),
>;

impl FeatureWorkspace {
    pub(super) fn tree_query(&self) -> ProjectionQuery {
        let mut query = self.filter.clone();
        query.archive = if self.include_archived {
            ArchiveFilter::All
        } else {
            ArchiveFilter::Active
        };
        query.tree = self.tree;
        query.collapsed = if self.tree {
            self.collapsed.iter().cloned().collect()
        } else {
            Vec::new()
        };
        ProjectionQuery::Features { query }
    }

    pub(super) fn tree_key(&mut self, key: crossterm::event::KeyCode) -> Result<()> {
        use crossterm::event::KeyCode;
        if key == KeyCode::Char('t') {
            self.tree = !self.tree;
            self.pending_tree = None;
        } else {
            let index = self.index.as_mut().ok_or_else(|| {
                PmError::new(ErrorCode::NotFound, "Feature reader is not available")
            })?;
            if index.querying || index.loading_page {
                let selected = index
                    .selected_row()
                    .map(|row| row.token.key.id.clone())
                    .or_else(|| self.selected.clone());
                self.pending_tree = selected.map(|id| (id, key));
                return Ok(());
            }
            let Some(row) = index.selected_row().cloned() else {
                return Ok(());
            };
            let id = row.token.key.id.parse::<workdeck_pm::FeatureId>()?;
            let children = row.tree.as_ref().map_or(0, |tree| tree.children);
            if key == KeyCode::Left {
                if children > 0 && !self.collapsed.contains(&id) {
                    self.collapsed.insert(id);
                } else if let Some(parent) = row.parent {
                    index.locate(parent);
                    return Ok(());
                } else {
                    return Ok(());
                }
            } else if !self.collapsed.remove(&id) {
                if children > 0 {
                    index.move_by(1);
                }
                return Ok(());
            }
        }
        let query = self.tree_query();
        if let Some(index) = &mut self.index {
            index.set_query(query);
        }
        Ok(())
    }

    pub(super) fn tree_label(&self, row: &ProjectionRow) -> String {
        let Some(position) = &row.tree else {
            return row.title.clone();
        };
        let closed = row
            .token
            .key
            .id
            .parse::<workdeck_pm::FeatureId>()
            .is_ok_and(|id| self.collapsed.contains(&id));
        let marker = if position.children == 0 {
            "·"
        } else if closed {
            "▸"
        } else {
            "▾"
        };
        let depth = if position.depth > 8 {
            format!("[depth {}] ", position.depth)
        } else {
            String::new()
        };
        format!(
            "{}{marker} {depth}{}{}",
            "  ".repeat(position.depth.min(8)),
            row.title,
            if position.parent_outside_view {
                " [parent outside view]"
            } else {
                ""
            }
        )
    }

    pub(super) fn refresh_index(&mut self) -> Result<()> {
        let query = self.tree_query();
        if self.index.is_none() {
            let root = self
                .repository()?
                .root()
                .parent()
                .ok_or_else(|| {
                    PmError::new(ErrorCode::InvalidInput, "Feature source has no worktree")
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
        let index = self.index.as_mut().expect("feature reader");
        if index.query != query {
            index.set_query(query);
        }
        index.refresh(false);
        self.attempted = None;
        self.coverage_requested = None;
        if let Some(worker) = &mut self.coverage_worker {
            worker
                .invalidate()
                .map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn poll_index(&mut self) {
        self.poll_coverage();
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
                    kind: SnapshotKind::Feature,
                    id,
                });
                return;
            }
        }
        let token = index.selected_row().map(|row| row.token.clone());
        if let Some((id, key)) = self.pending_tree.take() {
            if self.tree && token.as_ref().is_some_and(|token| token.key.id == id) {
                if let Err(error) = self.tree_key(key) {
                    self.error = Some(error);
                }
            } else {
                self.error = Some(PmError::new(
                    ErrorCode::StaleSource,
                    "Pending tree navigation canceled because its selected feature changed",
                ));
            }
            return;
        }
        if token.is_none()
            && index
                .handle
                .as_ref()
                .is_some_and(|handle| handle.total == 0)
        {
            self.records.clear();
            self.selected = None;
            self.coverage = None;
            self.attempted = None;
            self.coverage_requested = None;
            return;
        }
        if token == self.attempted {
            return;
        }
        self.attempted = token.clone();
        if let Some(token) = token
            && let Err(error) = self.queue_coverage(token)
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
                PmError::new(ErrorCode::NotFound, "Select an available indexed feature")
            })?;
        let repository = self.repository()?;
        let record = repository.feature_from_projection(&token)?;
        self.selected = Some(record.metadata.id.to_string());
        self.records = vec![record];
        self.attempted = Some(token.clone());
        self.queue_coverage(token)
    }

    fn queue_coverage(&mut self, token: ProjectionRowToken) -> Result<()> {
        if self.coverage_requested.as_ref() != Some(&token) {
            if self.coverage_worker.is_none() {
                let repository = self.repository()?.clone();
                self.coverage_worker = Some(
                    CoverageWorker::new(move |token: ProjectionRowToken| {
                        let result = (|| {
                            let before = repository.feature_from_projection(&token)?;
                            let coverage = repository
                                .feature_coverage(&FeatureCoverageQuery::new(&token.key.id))?;
                            let after = repository.feature_from_projection(&token)?;
                            if coverage.feature != before || coverage.feature != after {
                                return Err(PmError::new(
                                    ErrorCode::StaleSource,
                                    "Selected indexed feature changed while coverage was read",
                                )
                                .at(&token.path));
                            }
                            Ok(coverage)
                        })();
                        Ok((token, result))
                    })
                    .map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?,
                );
            }
            self.coverage_worker
                .as_mut()
                .expect("coverage reader")
                .request(
                    crate::workbench::projection_worker::ReadLane::Detail,
                    token.clone(),
                )
                .map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?;
            self.coverage_requested = Some(token);
            self.policy = None;
        }
        self.error = None;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn reader_idle(&self) -> bool {
        self.index.as_ref().is_none_or(|index| index.is_idle())
            && self
                .coverage_worker
                .as_ref()
                .is_none_or(|worker| worker.is_idle())
    }

    fn poll_coverage(&mut self) {
        let Some(worker) = &mut self.coverage_worker else {
            return;
        };
        while let Some(completion) = worker.poll() {
            match completion.result {
                Ok((token, result)) if self.coverage_requested.as_ref() == Some(&token) => {
                    self.coverage_requested = None;
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
                        Ok(coverage) => {
                            self.selected = Some(coverage.feature.metadata.id.to_string());
                            self.records = vec![coverage.feature.clone()];
                            self.coverage = Some(coverage);
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Err(error) => {
                    self.coverage_requested = None;
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
        Option<CoverageWorker>,
    ) {
        let mut index = self.index.take();
        let mut coverage = self.coverage_worker.take();
        if let Some(index) = &mut index {
            index.begin_shutdown();
        }
        if let Some(worker) = &mut coverage {
            worker.begin_shutdown();
        }
        (index, coverage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };
    use workdeck_pm::{CreateFeature, Repository, RequestId};

    fn fixture() -> (tempfile::TempDir, Repository, FeatureWorkspace) {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        for name in ["First", "Second"] {
            repository
                .create_feature(&CreateFeature::new(name), &RequestId::new())
                .unwrap();
        }
        let mut workspace = FeatureWorkspace::new_indexed(Some(repository.clone()));
        workspace.refresh().unwrap();
        settle(&mut workspace);
        (directory, repository, workspace)
    }
    fn settle(workspace: &mut FeatureWorkspace) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            workspace.poll_index();
            if workspace.reader_idle() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "reader did not settle: {:?}",
                workspace.error
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn feature_filter_keeps_retained_drafts_and_distinguishes_empty_results() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (_directory, _repository, mut workspace) = fixture();
        workspace.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        workspace.paste(" unsaved");
        let draft = workspace.active.clone().unwrap();
        let expected = workspace.drafts[&draft].expected.clone();
        workspace.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(
            workspace.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
            "slash must open native feature filtering"
        );
        workspace.paste("Second");
        workspace.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        settle(&mut workspace);
        assert_eq!(
            workspace
                .index
                .as_ref()
                .unwrap()
                .handle
                .as_ref()
                .unwrap()
                .total,
            1
        );
        assert_eq!(workspace.records[0].metadata.name, "Second");
        for query in ["no-such-capability", "First"] {
            workspace.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
            workspace.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
            workspace.paste(query);
            workspace.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
            settle(&mut workspace);
            if query == "no-such-capability" {
                assert_eq!(
                    workspace
                        .index
                        .as_ref()
                        .unwrap()
                        .handle
                        .as_ref()
                        .unwrap()
                        .total,
                    0
                );
                assert!(workspace.selected.is_none() && workspace.coverage.is_none());
            }
        }
        workspace.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(workspace.active.as_ref(), Some(&draft));
        assert_eq!(
            workspace.drafts[&draft].form.fields[0].value,
            "First unsaved"
        );
        assert_eq!(workspace.drafts[&draft].expected, expected);
    }

    #[test]
    fn native_tree_collapses_expands_and_returns_to_the_exact_parent() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (_directory, repository, mut workspace) = fixture();
        let parent = repository
            .list_features()
            .unwrap()
            .into_iter()
            .find(|record| record.metadata.name == "First")
            .unwrap();
        let mut input = CreateFeature::new("A child");
        input
            .fields
            .insert("parent".into(), serde_json::json!(parent.metadata.id));
        repository
            .create_feature(&input, &RequestId::new())
            .unwrap();
        workspace.refresh().unwrap();
        settle(&mut workspace);
        let selected = workspace.selected.clone();
        assert!(
            workspace.key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)),
            "t must open the native feature tree"
        );
        settle(&mut workspace);
        assert_eq!(workspace.selected, selected);
        workspace.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        settle(&mut workspace);
        assert_eq!(
            workspace.selected.as_deref(),
            Some(parent.metadata.id.as_str())
        );
        workspace.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        settle(&mut workspace);
        assert_eq!(
            workspace
                .index
                .as_ref()
                .unwrap()
                .handle
                .as_ref()
                .unwrap()
                .total,
            2
        );
        workspace.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        settle(&mut workspace);
        assert_eq!(
            workspace
                .index
                .as_ref()
                .unwrap()
                .handle
                .as_ref()
                .unwrap()
                .total,
            3
        );
        // A rapid reverse navigation must survive the pending collapse query.
        workspace.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        workspace.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        settle(&mut workspace);
        assert_eq!(
            workspace
                .index
                .as_ref()
                .unwrap()
                .handle
                .as_ref()
                .unwrap()
                .total,
            3
        );
        assert!(workspace.pending_tree.is_none());
        workspace.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        settle(&mut workspace);
        assert_eq!(workspace.records[0].metadata.name, "A child");
        workspace.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        settle(&mut workspace);
        assert_eq!(
            workspace.selected.as_deref(),
            Some(parent.metadata.id.as_str())
        );
        assert_eq!(workspace.records.len(), 1);
        workspace.key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        settle(&mut workspace);
        assert_eq!(
            workspace.selected.as_deref(),
            Some(parent.metadata.id.as_str())
        );
    }

    #[test]
    fn obsolete_coverage_error_cannot_replace_a_new_selection() {
        let (_directory, repository, mut workspace) = fixture();
        let old = workspace
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
        workspace.coverage_worker = Some(
            CoverageWorker::new(move |token: ProjectionRowToken| {
                if first {
                    first = false;
                    started_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok((
                        token,
                        Err(PmError::new(
                            ErrorCode::StaleSource,
                            "obsolete error must be discarded",
                        )),
                    ))
                } else {
                    let result =
                        repository.feature_coverage(&FeatureCoverageQuery::new(&token.key.id));
                    Ok((token, result))
                }
            })
            .unwrap(),
        );
        workspace.queue_coverage(old.clone()).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        workspace.index.as_mut().unwrap().end();
        let deadline = Instant::now() + Duration::from_secs(2);
        while workspace
            .coverage_requested
            .as_ref()
            .is_none_or(|token| token.key == old.key)
        {
            workspace.poll_index();
            assert!(
                Instant::now() < deadline,
                "new selection was blocked by old coverage"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let selected = workspace
            .coverage_requested
            .as_ref()
            .unwrap()
            .key
            .id
            .clone();
        assert!(workspace.error.is_none());
        release_tx.send(()).unwrap();
        settle(&mut workspace);
        assert!(workspace.error.is_none(), "{:?}", workspace.error);
        assert_eq!(
            workspace
                .coverage
                .as_ref()
                .unwrap()
                .feature
                .metadata
                .id
                .as_str(),
            selected
        );
        assert_eq!(workspace.selected.as_deref(), Some(selected.as_str()));
        assert_eq!(workspace.records.len(), 1);
    }

    #[test]
    fn current_coverage_error_retains_the_previous_inspected_result() {
        let (_directory, _repository, mut workspace) = fixture();
        let previous = workspace.coverage.clone();
        let token = workspace
            .index
            .as_ref()
            .unwrap()
            .selected_row()
            .unwrap()
            .token
            .clone();
        workspace.coverage_worker = Some(
            CoverageWorker::new(|token| {
                Ok((
                    token,
                    Err(PmError::new(
                        ErrorCode::StaleSource,
                        "source changed during coverage",
                    )),
                ))
            })
            .unwrap(),
        );
        workspace.queue_coverage(token).unwrap();
        settle(&mut workspace);
        assert_eq!(
            workspace.error.as_ref().unwrap().code,
            ErrorCode::StaleSource
        );
        assert_eq!(workspace.coverage, previous);
        assert!(workspace.coverage_requested.is_none());
    }

    #[test]
    fn feature_shutdown_retains_and_joins_inflight_coverage_ownership() {
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
        workspace.coverage_worker = Some(
            CoverageWorker::new(move |token| {
                started_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok((
                    token,
                    Err(PmError::new(ErrorCode::StaleSource, "shutdown fixture")),
                ))
            })
            .unwrap(),
        );
        workspace.queue_coverage(token).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let owners = workspace.take_index_shutdown();
        assert!(workspace.index.is_none() && workspace.coverage_worker.is_none());
        let (done_tx, done_rx) = mpsc::channel();
        let join = std::thread::spawn(move || {
            drop(owners);
            done_tx.send(()).unwrap();
        });
        assert!(
            matches!(
                done_rx.recv_timeout(Duration::from_millis(25)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "shutdown must not detach a still-running coverage reader"
        );
        release_tx.send(()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        join.join().unwrap();
    }
}
