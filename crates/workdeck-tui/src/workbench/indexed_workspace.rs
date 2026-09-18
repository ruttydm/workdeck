//! Compact read state. Refresh never rebases an opened citation or a draft.
use super::{
    projection_reader::{ProjectionRead, ProjectionReader, ProjectionReply, ProjectionValue},
    projection_worker::{ProjectionWorker, ReadLane},
    virtual_viewport::VirtualViewport,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{io, path::PathBuf};
use workdeck_pm::{ErrorCode, PmError, SourceSelector, projection::*};

#[derive(Debug)]
pub(super) struct IndexedWorkspace {
    worker: ProjectionWorker<ProjectionRead, ProjectionReply>,
    pub board: bool,
    pub board_group: IssueGroupBy,
    pub board_columns: Vec<ProjectionBoardColumn>,
    board_window: (usize, usize),
    board_requested: Option<(ProjectionQueryHandle, ProjectionBoardRequest)>,
    pub query: ProjectionQuery,
    pub handle: Option<ProjectionQueryHandle>,
    pub status: Option<ProjectionStatus>,
    pub groups: Vec<ProjectionGroup>,
    pub page: Option<ProjectionPage>,
    pub opened: Option<ProjectionDetail>,
    pub opened_lines: Vec<String>,
    pub detail_scroll: usize,
    pub error: Option<PmError>,
    pub worker_error: Option<String>,
    pub refreshing: bool,
    pub querying: bool,
    pub loading_page: bool,
    pub opening: Option<ProjectionRowToken>,
    viewport: VirtualViewport,
    query_view: Option<ProjectionViewId>,
    applied_query: Option<ProjectionQuery>,
    requested_page: Option<(ProjectionQueryHandle, usize, usize)>,
    page_limit: usize,
}

impl IndexedWorkspace {
    pub fn new(
        root: PathBuf,
        selector: SourceSelector,
        query: ProjectionQuery,
        limits: ProjectionLimits,
    ) -> io::Result<Self> {
        let page_limit = limits.max_page_rows;
        let mut reader = ProjectionReader::new(root, selector, limits);
        Self::with_reader(query, page_limit, move |request| Ok(reader.read(request)))
    }

    pub fn open(&mut self) {
        self.refreshing = true;
        self.send(ReadLane::Refresh, ProjectionRead::Open);
    }

    pub(super) fn with_reader(
        query: ProjectionQuery,
        page_limit: usize,
        reader: impl FnMut(ProjectionRead) -> Result<ProjectionReply, String> + Send + 'static,
    ) -> io::Result<Self> {
        if page_limit == 0 || page_limit > 10_000 {
            return Err(io::Error::other("Invalid projection page limit"));
        }
        Ok(Self {
            worker: ProjectionWorker::new(reader)?,
            board: false,
            board_group: IssueGroupBy::Status,
            board_columns: Vec::new(),
            board_window: (3, 6),
            board_requested: None,
            query,
            handle: None,
            status: None,
            groups: Vec::new(),
            page: None,
            opened: None,
            opened_lines: Vec::new(),
            detail_scroll: 0,
            error: None,
            worker_error: None,
            refreshing: false,
            querying: false,
            loading_page: false,
            opening: None,
            viewport: VirtualViewport::new(0, None, 0, 1),
            query_view: None,
            applied_query: None,
            requested_page: None,
            page_limit,
        })
    }

    pub fn refresh(&mut self, rebuild: bool) {
        self.refreshing = true;
        self.send(
            ReadLane::Refresh,
            ProjectionRead::Refresh(ProjectionRefreshRequest { rebuild }),
        );
    }

    pub fn set_query(&mut self, query: ProjectionQuery) {
        self.query = query;
        if let Some(view) = self
            .query_view
            .clone()
            .or_else(|| self.handle.as_ref().map(|handle| handle.view.clone()))
        {
            self.request_query(view);
        } else {
            self.refresh(false);
        }
    }

    fn request_query(&mut self, view: ProjectionViewId) {
        self.querying = true;
        self.query_view = Some(view.clone());
        self.send(
            ReadLane::Query,
            ProjectionRead::Query {
                view,
                query: Box::new(self.query.clone()),
                selected: self.selected_row().map(|row| row.token.key.clone()),
            },
        );
    }

    pub fn resize(&mut self, rows: u16) {
        // Keep page plus overscan within the core's declared response bound.
        self.viewport
            .resize(u64::from(rows).min(self.page_limit.saturating_sub(16).max(1) as u64));
        self.request_page();
    }
    pub fn home(&mut self) {
        self.viewport.home();
        self.request_page();
    }
    pub fn end(&mut self) {
        self.viewport.end();
        self.request_page();
    }
    pub fn move_by(&mut self, delta: i64) {
        self.viewport.move_by(delta);
        self.request_page();
    }
    pub fn move_page(&mut self, forward: bool) {
        self.viewport.page(forward);
        self.request_page();
    }
    pub fn locate(&mut self, key: ProjectionRecordKey) {
        if let Some(handle) = self.handle.clone() {
            self.send(ReadLane::Locate, ProjectionRead::Locate { handle, key });
        }
    }
    pub fn viewport(&self) -> &VirtualViewport {
        &self.viewport
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        if key.modifiers == KeyModifiers::SHIFT {
            match key.code {
                KeyCode::PageDown => {
                    self.detail_scroll = self
                        .detail_scroll
                        .saturating_add(10)
                        .min(self.opened_lines.len().saturating_sub(1))
                }
                KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(10),
                _ => return false,
            }
            return true;
        }
        match key.code {
            KeyCode::Char('w') if matches!(self.query, ProjectionQuery::Issues { .. }) => {
                self.board = !self.board;
                self.apply_board_group();
            }
            KeyCode::Char('z') if self.board => {
                self.board_group = match self.board_group {
                    IssueGroupBy::Status => IssueGroupBy::Priority,
                    IssueGroupBy::Priority => IssueGroupBy::Assignee,
                    IssueGroupBy::Assignee => IssueGroupBy::Project,
                    IssueGroupBy::Project => IssueGroupBy::Cycle,
                    IssueGroupBy::Cycle => IssueGroupBy::Milestone,
                    IssueGroupBy::Milestone => IssueGroupBy::Status,
                };
                self.apply_board_group();
            }
            KeyCode::Left if self.board => self.move_column(false),
            KeyCode::Right if self.board => self.move_column(true),
            KeyCode::Home => self.home(),
            KeyCode::End => self.end(),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_page(false),
            KeyCode::PageDown => self.move_page(true),
            KeyCode::Enter => self.open_selected(),
            KeyCode::Char('r') => self.refresh(false),
            _ => return false,
        }
        true
    }

    pub fn selected_row(&self) -> Option<&ProjectionRow> {
        let ordinal = usize::try_from(self.viewport.selected()?).ok()?;
        self.row_at(ordinal)
    }

    pub fn row_at(&self, ordinal: usize) -> Option<&ProjectionRow> {
        self.page
            .iter()
            .chain(self.board_columns.iter().map(|column| &column.page))
            .filter(|page| Some(&page.handle) == self.handle.as_ref())
            .find_map(|page| page.rows.get(ordinal.checked_sub(page.offset)?))
    }

    pub fn board_ready(&self) -> bool {
        !self.querying && self.applied_query.as_ref() == Some(&self.query)
    }

    pub fn select_token(&mut self, token: &ProjectionRowToken) -> bool {
        let ordinal = self
            .page
            .iter()
            .chain(self.board_columns.iter().map(|column| &column.page))
            .filter(|page| Some(&page.handle) == self.handle.as_ref())
            .find_map(|page| {
                page.rows
                    .iter()
                    .position(|row| &row.token == token)
                    .map(|index| page.offset + index)
            });
        let Some(ordinal) = ordinal else {
            return false;
        };
        self.viewport.select(ordinal as u64);
        self.request_page();
        true
    }

    pub fn visible_rows(&self) -> impl Iterator<Item = (usize, &ProjectionRow)> {
        let range = self.viewport.visible();
        self.page
            .iter()
            .filter(|page| Some(&page.handle) == self.handle.as_ref())
            .flat_map(move |page| {
                let range = range.clone();
                page.rows
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, row)| {
                        let ordinal = page.offset + index;
                        range.contains(&(ordinal as u64)).then_some((ordinal, row))
                    })
            })
    }

    /// Enter is explicit. Selection changes and refreshed pages leave this
    /// exact row token, request and eventual source excerpt alone.
    pub fn open_selected(&mut self) {
        let Some(token) = self.selected_row().map(|row| row.token.clone()) else {
            return;
        };
        self.opening = Some(token.clone());
        self.send(ReadLane::Detail, ProjectionRead::Detail(token));
    }

    pub fn stale(&self) -> bool {
        self.worker_error.is_some()
            || self.handle.is_some() && self.applied_query.as_ref() != Some(&self.query)
            || self.status.as_ref().is_some_and(|status| {
                matches!(
                    status.state,
                    ProjectionState::Cached | ProjectionState::Stale | ProjectionState::Error
                ) || self
                    .handle
                    .as_ref()
                    .is_some_and(|handle| status.view.as_ref() != Some(&handle.view))
            })
    }

    pub fn poll(&mut self) -> bool {
        let Some(completion) = self.worker.poll() else {
            if let Some(error) = self.worker.failure() {
                let changed = self.worker_error.as_deref() != Some(error);
                self.worker_error = Some(error.into());
                self.stop_loading();
                return changed;
            }
            return false;
        };
        match completion.ticket.lane {
            ReadLane::Refresh => self.refreshing = false,
            ReadLane::Query => self.querying = false,
            ReadLane::Page => self.loading_page = false,
            _ => {}
        }
        match completion.result {
            Err(error) => {
                self.worker_error = Some(error);
                self.stop_loading();
            }
            Ok(reply) => {
                self.worker_error = None;
                self.status = Some(reply.status);
                match reply.result {
                    Err(error) => {
                        if completion.ticket.lane == ReadLane::Refresh
                            && self.handle.is_none()
                            && let Some(view) = reply.retained
                        {
                            self.request_query(view);
                        }
                        if completion.ticket.lane == ReadLane::Detail {
                            self.opening = None;
                        }
                        if completion.ticket.lane == ReadLane::Page {
                            self.requested_page = None;
                        }
                        self.error = Some(error);
                    }
                    Ok(value) => {
                        if let Err(error) = self.accept(value) {
                            if completion.ticket.lane == ReadLane::Detail {
                                self.opening = None;
                            }
                            if completion.ticket.lane == ReadLane::Page {
                                self.requested_page = None;
                            }
                            self.error = Some(error);
                        } else {
                            self.error = None;
                        }
                    }
                }
            }
        }
        if let Some(error) = self.worker.failure() {
            self.worker_error = Some(error.into());
            self.stop_loading();
        }
        true
    }

    fn accept(&mut self, value: ProjectionValue) -> Result<(), PmError> {
        match value {
            ProjectionValue::Refreshed(view) => {
                if self.handle.as_ref().is_some_and(|handle| {
                    handle.view.slot != view.slot
                        || handle.view.source.repository != view.source.repository
                }) {
                    return Err(stale(
                        "Refresh changed the selected repository or source slot",
                    ));
                }
                self.request_query(view);
            }
            ProjectionValue::Query {
                handle,
                groups,
                located,
            } => {
                if self.query_view.as_ref() != Some(&handle.view) {
                    return Err(stale(
                        "Query response belongs to another projection generation",
                    ));
                }
                self.viewport
                    .replace(handle.total as u64, located.map(|index| index as u64));
                self.handle = Some(handle);
                self.applied_query = Some(self.query.clone());
                self.groups = groups;
                self.board_columns.clear();
                self.board_requested = None;
                self.page = None;
                self.requested_page = None;
                self.request_page();
            }
            ProjectionValue::Board {
                handle,
                request,
                columns,
            } => {
                if self.handle.as_ref() != Some(&handle)
                    || self.board_requested.as_ref() != Some(&(handle.clone(), request.clone()))
                {
                    return Ok(());
                }
                let expected = self
                    .groups
                    .iter()
                    .skip(request.first_group)
                    .take(request.columns);
                let mut start = self
                    .groups
                    .iter()
                    .take(request.first_group)
                    .map(|group| group.count)
                    .sum::<usize>();
                if columns.len() != expected.len()
                    || columns.iter().zip(expected).any(|(column, group)| {
                        let local = request
                            .selected
                            .filter(|selected| {
                                *selected >= start && *selected < start + group.count
                            })
                            .map_or(0, |selected| selected - start);
                        let offset = local.saturating_add(1).saturating_sub(request.rows);
                        let invalid = &column.group != group
                            || column.page.handle != handle
                            || column.page.offset != start + offset
                            || column.page.rows.len() != request.rows.min(group.count - offset)
                            || column.page.rows.iter().any(|row| {
                                row.token.view != handle.view
                                    || row.token.key.repository != handle.view.source.repository
                                    || row.group != column.group.value
                            });
                        start += group.count;
                        invalid
                    })
                {
                    return Err(stale(
                        "Board response differs from its captured column window",
                    ));
                }
                self.board_columns = columns;
            }
            ProjectionValue::Page(page) => {
                if self.handle.as_ref() != Some(&page.handle) {
                    return Ok(());
                }
                let Some((handle, offset, limit)) = &self.requested_page else {
                    return Ok(());
                };
                if &page.handle != handle
                    || page.offset != *offset
                    || page.rows.len() != (*limit).min(handle.total.saturating_sub(*offset))
                    || page.offset.saturating_add(page.rows.len()) > handle.total
                    || page.rows.iter().any(|row| {
                        row.token.view != handle.view
                            || row.token.key.repository != handle.view.source.repository
                    })
                {
                    return Err(stale(
                        "Page response does not match the inspected query window",
                    ));
                }
                self.requested_page = None;
                let visible = self.viewport.visible();
                if page.offset as u64 <= visible.start
                    && page.offset.saturating_add(page.rows.len()) as u64 >= visible.end
                {
                    self.page = Some(page);
                }
                self.request_page();
            }
            ProjectionValue::Located {
                handle,
                key,
                ordinal,
            } => {
                if self.handle.as_ref() != Some(&handle) {
                    return Ok(());
                }
                if key.repository != handle.view.source.repository {
                    return Err(stale("Located record belongs to another repository"));
                }
                if let Some(ordinal) = ordinal {
                    if ordinal >= handle.total {
                        return Err(stale("Located ordinal is outside its query"));
                    }
                    self.viewport.select(ordinal as u64);
                    self.request_page();
                } else {
                    return Err(PmError::new(
                        ErrorCode::NotFound,
                        "Record is outside this captured query",
                    ));
                }
            }
            ProjectionValue::Detail(detail) => {
                if self.opening.as_ref() != Some(&detail.row.token) {
                    return Err(stale(
                        "Detail response differs from the exact opened row token",
                    ));
                }
                self.opened_lines = detail
                    .document
                    .as_deref()
                    .unwrap_or_default()
                    .lines()
                    .map(workdeck_diff::sanitize_terminal_line)
                    .collect();
                self.detail_scroll = 0;
                self.opened = Some(*detail);
                self.opening = None;
            }
        }
        Ok(())
    }

    fn apply_board_group(&mut self) {
        let ProjectionQuery::Issues { query, .. } = &self.query else {
            return;
        };
        self.set_query(ProjectionQuery::Issues {
            query: query.clone(),
            group_by: self.board.then_some(self.board_group),
        });
    }

    pub fn resize_board(&mut self, columns: usize, rows: usize) {
        let columns = columns.clamp(1, 8).min(self.page_limit);
        self.board_window = (columns, rows.max(1).min(self.page_limit / columns));
        self.request_board();
    }

    fn selected_group(&self) -> (usize, usize) {
        let selected = self.viewport.selected().unwrap_or(0) as usize;
        let mut start = 0;
        for (index, group) in self.groups.iter().enumerate() {
            if selected < start + group.count {
                return (index, start);
            }
            start += group.count;
        }
        (0, 0)
    }

    fn move_column(&mut self, forward: bool) {
        let (index, start) = self.selected_group();
        if forward {
            if let Some(group) = self.groups.get(index)
                && index + 1 < self.groups.len()
            {
                self.viewport.select((start + group.count) as u64);
            }
        } else if index > 0 {
            self.viewport
                .select((start - self.groups[index - 1].count) as u64);
        }
        self.request_page();
    }

    fn request_board(&mut self) {
        if !self.board || self.querying {
            return;
        }
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let (selected_group, _) = self.selected_group();
        let request = ProjectionBoardRequest {
            first_group: selected_group
                .saturating_sub(self.board_window.0 / 2)
                .min(self.groups.len().saturating_sub(self.board_window.0)),
            columns: self.board_window.0,
            rows: self.board_window.1,
            selected: self.viewport.selected().map(|selected| selected as usize),
        };
        if self.board_requested.as_ref() == Some(&(handle.clone(), request.clone())) {
            return;
        }
        self.board_requested = Some((handle.clone(), request.clone()));
        self.send(ReadLane::Board, ProjectionRead::Board { handle, request });
    }

    fn request_page(&mut self) {
        self.request_board();
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let visible = self.viewport.visible();
        if visible.is_empty() {
            return;
        }
        if self.page.as_ref().is_some_and(|page| {
            page.handle == handle
                && page.offset as u64 <= visible.start
                && page.offset.saturating_add(page.rows.len()) as u64 >= visible.end
        }) {
            return;
        }
        let overscan = (self.page_limit as u64).saturating_sub(visible.end - visible.start) / 2;
        let range = self.viewport.requested(overscan);
        let offset = range.start as usize;
        let limit = (range.end - range.start).min(self.page_limit as u64) as usize;
        let request = (handle.clone(), offset, limit);
        if self.requested_page.as_ref() == Some(&request) {
            return;
        }
        self.requested_page = Some(request);
        self.loading_page = true;
        self.send(
            ReadLane::Page,
            ProjectionRead::Page {
                handle,
                offset,
                limit,
            },
        );
    }

    fn send(&mut self, lane: ReadLane, request: ProjectionRead) {
        if let Err(error) = self.worker.request(lane, request) {
            self.worker_error = Some(error.to_string());
            self.stop_loading();
        }
    }
    fn stop_loading(&mut self) {
        self.refreshing = false;
        self.querying = false;
        self.loading_page = false;
        self.opening = None;
    }
    pub(super) fn is_idle(&self) -> bool {
        self.worker.is_idle()
    }

    pub fn begin_shutdown(&mut self) {
        self.worker.begin_shutdown();
        self.stop_loading();
    }
    #[cfg(test)]
    pub fn finish_shutdown(&mut self) -> bool {
        self.worker.finish_shutdown()
    }
}

fn stale(message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
