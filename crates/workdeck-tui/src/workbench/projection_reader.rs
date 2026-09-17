//! Typed worker adapter. All filesystem/SQLite access remains in the shared PM API.
use std::{collections::VecDeque, path::PathBuf};
use workdeck_pm::{ErrorCode, PmError, SourceSelector, projection::*};

#[derive(Debug, Clone)]
pub(super) enum ProjectionRead {
    Open,
    Refresh(ProjectionRefreshRequest),
    Query {
        view: ProjectionViewId,
        query: Box<ProjectionQuery>,
        selected: Option<ProjectionRecordKey>,
    },
    Page {
        handle: ProjectionQueryHandle,
        offset: usize,
        limit: usize,
    },
    Locate {
        handle: ProjectionQueryHandle,
        key: ProjectionRecordKey,
    },
    Board {
        handle: ProjectionQueryHandle,
        request: ProjectionBoardRequest,
    },
    Detail(ProjectionRowToken),
}

#[derive(Debug)]
pub(super) enum ProjectionValue {
    Refreshed(ProjectionViewId),
    Query {
        handle: ProjectionQueryHandle,
        groups: Vec<ProjectionGroup>,
        located: Option<usize>,
    },
    Page(ProjectionPage),
    Board {
        handle: ProjectionQueryHandle,
        request: ProjectionBoardRequest,
        columns: Vec<ProjectionBoardColumn>,
    },
    Located {
        handle: ProjectionQueryHandle,
        key: ProjectionRecordKey,
        ordinal: Option<usize>,
    },
    Detail(Box<ProjectionDetail>),
}

#[derive(Debug)]
pub(super) struct ProjectionReply {
    pub status: ProjectionStatus,
    pub retained: Option<ProjectionViewId>,
    pub result: Result<ProjectionValue, PmError>,
}

pub(super) struct ProjectionReader {
    root: PathBuf,
    selector: SourceSelector,
    limits: ProjectionLimits,
    store: Option<ProjectionStore>,
    // Opened excerpts live independently in the UI. Old paged queries retain
    // one previous generation; older evicted tokens fail explicitly.
    views: VecDeque<Box<ProjectionReadView>>,
}

impl ProjectionReader {
    pub fn new(root: PathBuf, selector: SourceSelector, limits: ProjectionLimits) -> Self {
        Self {
            root,
            selector,
            limits,
            store: None,
            views: VecDeque::new(),
        }
    }

    pub fn read(&mut self, request: ProjectionRead) -> ProjectionReply {
        let result = self.execute(request);
        let status = self
            .store
            .as_ref()
            .map(|store| store.status().clone())
            .unwrap_or_else(|| ProjectionStatus {
                state: ProjectionState::Error,
                view: None,
                observation: None,
                publication_binding: None,
                diagnostics: result.as_ref().err().cloned().into_iter().collect(),
            });
        ProjectionReply {
            status,
            retained: self.views.back().map(|view| view.id().clone()),
            result,
        }
    }

    fn execute(&mut self, request: ProjectionRead) -> Result<ProjectionValue, PmError> {
        if matches!(request, ProjectionRead::Open) {
            if let Some(view) = self.views.back() {
                return Ok(ProjectionValue::Refreshed(view.id().clone()));
            }
            return self.execute(ProjectionRead::Refresh(ProjectionRefreshRequest {
                rebuild: false,
            }));
        }
        if self.store.is_none() {
            self.store = Some(ProjectionStore::open(
                &self.root,
                self.selector.clone(),
                self.limits.clone(),
            )?);
            // An invalid disposable cache can be rebuilt by refresh. A valid
            // old cache remains useful when current source parsing then fails.
            if let Ok(Some(cached)) = self.store.as_mut().expect("opened store").load() {
                self.retain(Box::new(cached));
            }
        }
        match request {
            ProjectionRead::Open => unreachable!("handled before opening store"),
            ProjectionRead::Refresh(request) => {
                let result = self
                    .store
                    .as_mut()
                    .expect("opened store")
                    .refresh(&request)?;
                let id = match result {
                    ProjectionRefresh::Published(view) => {
                        let id = view.id().clone();
                        self.retain(view);
                        id
                    }
                    ProjectionRefresh::Unchanged(id) | ProjectionRefresh::Superseded(id) => {
                        if !self.views.iter().any(|view| view.id() == &id) {
                            let loaded = self
                                .store
                                .as_mut()
                                .expect("opened store")
                                .load()?
                                .ok_or_else(|| stale("Published projection is unavailable"))?;
                            if loaded.id() != &id {
                                return Err(stale(
                                    "Projection publication changed before loading its generation",
                                ));
                            }
                            self.retain(Box::new(loaded));
                        }
                        id
                    }
                };
                Ok(ProjectionValue::Refreshed(id))
            }
            ProjectionRead::Query {
                view,
                query,
                selected,
            } => {
                let view = self.view(&view)?;
                let handle = view.query(&query)?;
                let groups = view.groups(&handle)?;
                let located = selected
                    .as_ref()
                    .map(|key| view.locate(&handle, key))
                    .transpose()?
                    .flatten();
                Ok(ProjectionValue::Query {
                    handle,
                    groups,
                    located,
                })
            }
            ProjectionRead::Page {
                handle,
                offset,
                limit,
            } => Ok(ProjectionValue::Page(
                self.view(&handle.view)?.page(&handle, offset, limit)?,
            )),
            ProjectionRead::Board { handle, request } => {
                let columns = self.view(&handle.view)?.board(&handle, &request)?;
                Ok(ProjectionValue::Board {
                    handle,
                    request,
                    columns,
                })
            }
            ProjectionRead::Locate { handle, key } => {
                let ordinal = self.view(&handle.view)?.locate(&handle, &key)?;
                Ok(ProjectionValue::Located {
                    handle,
                    key,
                    ordinal,
                })
            }
            ProjectionRead::Detail(token) => Ok(ProjectionValue::Detail(Box::new(
                self.view(&token.view)?.detail(&token)?,
            ))),
        }
    }

    fn retain(&mut self, view: Box<ProjectionReadView>) {
        self.views.retain(|old| old.id() != view.id());
        self.views.push_back(view);
        while self.views.len() > 2 {
            self.views.pop_front();
        }
    }

    fn view(&self, id: &ProjectionViewId) -> Result<&ProjectionReadView, PmError> {
        self.views.iter().find(|view| view.id() == id).map(Box::as_ref).ok_or_else(|| stale("This projection generation is no longer retained; the opened citation is unchanged"))
    }
}

fn stale(message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
