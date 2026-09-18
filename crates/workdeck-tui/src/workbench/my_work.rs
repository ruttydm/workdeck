//! Read-only cross-checkout observations. Native drafts remain in their source controller.
#[path = "my_work_evidence.rs"]
mod evidence_view;
use super::{
    input::{FormAction, FormKind, TextField, WorkbenchForm},
    projection_worker::{ProjectionWorker, ReadLane},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use workdeck_pm::{
    ErrorCode, PmError, Repository,
    projection::{ProjectionDetail, ProjectionLimits, ProjectionRowToken, ProjectionStore},
    registry::{MyWorkFacet, MyWorkReport, MyWorkRequest, RegisteredCheckout, RegistryStore},
};

#[derive(Debug, Clone)]
enum Read {
    Navigate(RegisteredCheckout),
    Report {
        input: MyWorkRequest,
        previous: Vec<Option<String>>,
        form: Option<Vec<String>>,
    },
    Detail {
        checkout: RegisteredCheckout,
        token: Box<ProjectionRowToken>,
    },
}
#[derive(Debug)]
enum Value {
    Navigate(workdeck_pm::registry::RegistryNavigation),
    Report {
        input: MyWorkRequest,
        previous: Vec<Option<String>>,
        form: Option<Vec<String>>,
        report: Box<MyWorkReport>,
    },
    Detail {
        checkout: RegisteredCheckout,
        detail: Box<ProjectionDetail>,
    },
}

#[derive(Debug)]
pub(super) struct MyWorkWorkspace {
    pub show_evidence: bool,
    evidence_lines: Vec<String>,
    evidence_width: u16,
    evidence_scroll: usize,
    pub ready_navigation: Option<workdeck_pm::registry::RegistryNavigation>,
    worker: Option<ProjectionWorker<Read, Result<Value, PmError>>>,
    input: MyWorkRequest,
    previous: Vec<Option<String>>,
    pub report: Option<MyWorkReport>,
    pub selected: usize,
    pub source_selected: usize,
    pub sources: bool,
    pub form: Option<WorkbenchForm>,
    pub error: Option<String>,
    pub opened: Option<(RegisteredCheckout, Box<ProjectionDetail>)>,
    pub opened_lines: Vec<String>,
    pub detail_scroll: usize,
    pub page_rows: usize,
    pub list_bounds: Rect,
    pub detail_bounds: Rect,
    pub hits: Vec<(Rect, usize)>,
}

impl MyWorkWorkspace {
    pub fn new(
        root: std::path::PathBuf,
        owner: Result<Repository, PmError>,
        assignee: String,
    ) -> Self {
        // Construction performs no I/O on the render/input thread. The store pins
        // its owner on first read and revalidates it for every later operation.
        let mut store = None;
        let worker = ProjectionWorker::new(move |request| {
            let result = (|| {
                if store.is_none() {
                    // An unavailable startup may discover its first source after
                    // explicit external initialization. Once opened, the store
                    // keeps its original owner binding through all later errors.
                    let repository = owner.clone().or_else(|_| Repository::discover(&root))?;
                    store = Some(RegistryStore::open(&repository)?);
                }
                let store = store.as_ref().expect("opened registry");
                match request {
                    Read::Navigate(checkout) => {
                        Ok(Value::Navigate(store.prepare_navigation(&checkout)?))
                    }
                    Read::Report {
                        input,
                        previous,
                        form,
                    } => Ok(Value::Report {
                        report: Box::new(store.my_work(&input)?),
                        input,
                        previous,
                        form,
                    }),
                    Read::Detail { checkout, token } => {
                        let current = store.resolve(&checkout.alias)?.0;
                        if current != checkout {
                            return Err(PmError::new(
                                ErrorCode::StaleSource,
                                "Registered checkout changed after this row was inspected",
                            ));
                        }
                        let mut cache = ProjectionStore::open_cached(
                            &checkout.checkout,
                            checkout.source.clone(),
                            ProjectionLimits::default(),
                        )?;
                        let view = cache.load()?.ok_or_else(|| {
                            PmError::new(
                                ErrorCode::NotFound,
                                "The inspected cached source is unavailable; refresh My work",
                            )
                        })?;
                        let detail = view.detail(&token)?;
                        if store.resolve(&checkout.alias)?.0 != checkout {
                            return Err(PmError::new(
                                ErrorCode::StaleSource,
                                "Registered checkout changed while opening its source",
                            ));
                        }
                        Ok(Value::Detail {
                            checkout,
                            detail: Box::new(detail),
                        })
                    }
                }
            })();
            Ok(result)
        });
        let (worker, error) = match worker {
            Ok(worker) => (Some(worker), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let mut workspace = Self {
            show_evidence: false,
            evidence_lines: Vec::new(),
            evidence_width: 0,
            evidence_scroll: 0,
            ready_navigation: None,
            worker,
            error,
            input: MyWorkRequest {
                assignee,
                ..Default::default()
            },
            previous: Vec::new(),
            report: None,
            selected: 0,
            source_selected: 0,
            sources: false,
            form: None,
            opened: None,
            opened_lines: Vec::new(),
            detail_scroll: 0,
            page_rows: 10,
            list_bounds: Rect::default(),
            detail_bounds: Rect::default(),
            hits: Vec::new(),
        };
        workspace.refresh();
        workspace
    }
    fn request(&mut self, lane: ReadLane, input: Read) {
        if let Some(worker) = &mut self.worker {
            match worker.request(lane, input) {
                Ok(_) => self.error = None,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
    pub fn refresh(&mut self) {
        let mut input = self.input.clone();
        input.cursor = None;
        input.as_of = matches!(input.facet, MyWorkFacet::Overdue | MyWorkFacet::Claimed)
            .then(chrono::Utc::now);
        self.request(
            ReadLane::Query,
            Read::Report {
                input,
                previous: Vec::new(),
                form: None,
            },
        );
    }
    fn select_facet(&mut self, facet: MyWorkFacet) {
        let mut input = self.input.clone();
        input.facet = facet;
        input.cursor = None;
        input.as_of =
            matches!(facet, MyWorkFacet::Overdue | MyWorkFacet::Claimed).then(chrono::Utc::now);
        self.sources = false;
        self.hits.clear();
        self.request(
            ReadLane::Query,
            Read::Report {
                input,
                previous: Vec::new(),
                form: None,
            },
        );
    }
    pub fn is_idle(&self) -> bool {
        self.worker.as_ref().is_none_or(|worker| worker.is_idle())
    }
    pub fn begin_shutdown(&mut self) {
        if let Some(worker) = &mut self.worker {
            worker.begin_shutdown();
        }
    }
    pub fn poll(&mut self) {
        let result = self.worker.as_mut().and_then(|worker| worker.poll());
        if let Some(result) = result {
            match result
                .result
                .map_err(|error| PmError::new(ErrorCode::Io, error))
                .and_then(|result| result)
            {
                Ok(Value::Navigate(navigation)) => self.ready_navigation = Some(navigation),
                Ok(Value::Report {
                    input,
                    previous,
                    form,
                    report,
                }) => {
                    let old = self
                        .report
                        .as_ref()
                        .and_then(|report| report.rows.get(self.selected))
                        .map(|row| (&row.alias, &row.row.token.key, &row.row.token.view.slot));
                    self.selected = old
                        .and_then(|(alias, key, slot)| {
                            report.rows.iter().position(|row| {
                                &row.alias == alias
                                    && &row.row.token.key == key
                                    && &row.row.token.view.slot == slot
                            })
                        })
                        .unwrap_or(0);
                    let selected_source = self
                        .report
                        .as_ref()
                        .and_then(|report| report.sources.get(self.source_selected))
                        .map(|source| &source.checkout);
                    self.source_selected = selected_source
                        .and_then(|selected| {
                            report
                                .sources
                                .iter()
                                .position(|source| &source.checkout == selected)
                        })
                        .unwrap_or(0);
                    // Coordinates belong to the last rendered report. Invalidate
                    // them before publishing a different observation; the next
                    // paint installs hit targets for the newly visible rows.
                    self.hits.clear();
                    self.clear_evidence();
                    self.input = input;
                    self.previous = previous;
                    self.report = Some(*report);
                    if let (Some(expected), Some(current)) = (form, &self.form)
                        && current
                            .fields
                            .iter()
                            .map(|field| &field.value)
                            .eq(expected.iter())
                    {
                        self.form = None;
                    }
                    self.error = None;
                }
                Ok(Value::Detail { checkout, detail }) => {
                    let document = detail
                        .document
                        .as_deref()
                        .unwrap_or("No source excerpt available");
                    self.opened_lines = document
                        .lines()
                        .map(workdeck_diff::sanitize_terminal_line)
                        .collect();
                    if detail.omitted_document_bytes > 0 {
                        self.opened_lines.push(format!(
                            "{} source bytes omitted",
                            detail.omitted_document_bytes
                        ));
                    }
                    self.opened = Some((checkout, detail));
                    self.detail_scroll = 0;
                    self.error = None;
                }
                Err(error) => self.error = Some(error.message),
            }
        }
        if let Some(error) = self.worker.as_ref().and_then(|worker| worker.failure()) {
            self.error = Some(error.into());
        }
    }
    fn prepare_navigation(&mut self) {
        let Some(report) = &self.report else {
            return;
        };
        let source = if self.sources {
            report.sources.get(self.source_selected)
        } else {
            report.rows.get(self.selected).and_then(|row| {
                report
                    .sources
                    .iter()
                    .find(|source| source.checkout.alias == row.alias)
            })
        };
        let Some(source) = source else {
            return;
        };
        let checkout = source.checkout.clone();
        self.ready_navigation = None;
        self.request(ReadLane::Locate, Read::Navigate(checkout));
    }

    fn open_selected(&mut self) {
        self.show_evidence = false;
        let Some(report) = &self.report else {
            return;
        };
        let Some(row) = report.rows.get(self.selected) else {
            return;
        };
        let Some(source) = report
            .sources
            .iter()
            .find(|source| source.checkout.alias == row.alias)
        else {
            return;
        };
        self.request(
            ReadLane::Detail,
            Read::Detail {
                checkout: source.checkout.clone(),
                token: Box::new(row.row.token.clone()),
            },
        );
    }
    fn length(&self) -> usize {
        self.report.as_ref().map_or(0, |report| {
            if self.sources {
                report.sources.len()
            } else {
                report.rows.len()
            }
        })
    }
    fn move_by(&mut self, delta: isize) {
        self.clear_evidence();
        let length = self.length();
        let selected = if self.sources {
            &mut self.source_selected
        } else {
            &mut self.selected
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(length.saturating_sub(1));
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if let Some(form) = &mut self.form
            && let Some(field) = form.fields.get_mut(form.selected)
        {
            field.insert(text);
        }
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return false;
        }
        if let Some(form) = &mut self.form {
            match form.key(key) {
                FormAction::Submit => {
                    let values = form
                        .fields
                        .iter()
                        .map(|field| field.value.clone())
                        .collect::<Vec<_>>();
                    let input = MyWorkRequest {
                        assignee: values[0].clone(),
                        aliases: values[1].split_whitespace().map(str::to_owned).collect(),
                        ..self.input.clone()
                    };
                    self.request(
                        ReadLane::Query,
                        Read::Report {
                            input: MyWorkRequest {
                                cursor: None,
                                ..input
                            },
                            previous: Vec::new(),
                            form: Some(values),
                        },
                    );
                }
                FormAction::Close => self.form = None,
                FormAction::Edited => (),
            }
            return true;
        }
        if key.modifiers == KeyModifiers::SHIFT && self.show_evidence {
            match key.code {
                KeyCode::PageDown => self.scroll_evidence(self.page_rows as isize),
                KeyCode::PageUp => self.scroll_evidence(-(self.page_rows as isize)),
                _ => return false,
            }
            return true;
        }
        if key.modifiers == KeyModifiers::SHIFT {
            match key.code {
                KeyCode::PageDown => {
                    self.detail_scroll = self
                        .detail_scroll
                        .saturating_add(self.page_rows)
                        .min(self.opened_lines.len().saturating_sub(1))
                }
                KeyCode::PageUp => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(self.page_rows)
                }
                _ => return false,
            }
            return true;
        }
        if !key.modifiers.is_empty() {
            return false;
        }
        let prior_selected = self.selected;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('1') => self.select_facet(MyWorkFacet::Assigned),
            KeyCode::Char('2') => self.select_facet(MyWorkFacet::ReviewRequested),
            KeyCode::Char('3') => self.select_facet(MyWorkFacet::Overdue),
            KeyCode::Char('4') => self.select_facet(MyWorkFacet::Blocked),
            KeyCode::Char('5') => self.select_facet(MyWorkFacet::Claimed),
            KeyCode::Char('e') => {
                self.show_evidence = !self.show_evidence;
                self.clear_evidence();
            }
            KeyCode::Char('o') => self.prepare_navigation(),
            KeyCode::Char('/') => {
                self.form = Some(WorkbenchForm::new(
                    FormKind::Planning,
                    "My work filter",
                    vec![
                        TextField::new(
                            "Actor (assignee or requested reviewer)",
                            self.input.assignee.clone(),
                            false,
                        ),
                        TextField::new(
                            "Checkout aliases (blank means all registered)",
                            self.input.aliases.join("\n"),
                            true,
                        ),
                    ],
                ))
            }
            KeyCode::Char('s') => {
                self.sources = !self.sources;
                self.hits.clear();
            }
            KeyCode::Enter if !self.sources => self.open_selected(),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::PageDown => self.move_by(self.page_rows as isize),
            KeyCode::PageUp => self.move_by(-(self.page_rows as isize)),
            KeyCode::Home => self.move_by(-(self.length() as isize)),
            KeyCode::End => self.move_by(self.length() as isize),
            KeyCode::Char(']') if !self.sources => {
                if let Some(cursor) = self
                    .report
                    .as_ref()
                    .and_then(|report| report.next_cursor.clone())
                {
                    let mut previous = self.previous.clone();
                    // Keep navigation memory bounded independently of total matches.
                    if previous.len() == 128 {
                        previous.remove(0);
                    }
                    // Returning to page zero is still navigation within this
                    // report, not permission to silently capture a new generation.
                    let current = self.input.cursor.clone().or_else(|| {
                        cursor
                            .split_once(':')
                            .map(|(fingerprint, _)| format!("{fingerprint}:0"))
                    });
                    previous.push(current);
                    self.request(
                        ReadLane::Query,
                        Read::Report {
                            input: MyWorkRequest {
                                cursor: Some(cursor),
                                ..self.input.clone()
                            },
                            previous,
                            form: None,
                        },
                    );
                }
            }
            KeyCode::Char('[') if !self.sources => {
                let mut previous = self.previous.clone();
                if let Some(cursor) = previous.pop() {
                    self.request(
                        ReadLane::Query,
                        Read::Report {
                            input: MyWorkRequest {
                                cursor,
                                ..self.input.clone()
                            },
                            previous,
                            form: None,
                        },
                    );
                }
            }
            _ => (),
        }
        if prior_selected != self.selected {
            self.clear_evidence();
        }
        true
    }
    pub fn mouse(&mut self, event: &MouseEvent) -> bool {
        let point = (event.column, event.row).into();
        if self.form.is_some() {
            return true;
        }
        if self.detail_bounds.contains(point) && self.show_evidence && !self.sources {
            match event.kind {
                MouseEventKind::ScrollDown => self.scroll_evidence(3),
                MouseEventKind::ScrollUp => self.scroll_evidence(-3),
                _ => (),
            }
            return true;
        }
        if self.detail_bounds.contains(point) {
            match event.kind {
                MouseEventKind::ScrollDown => {
                    self.detail_scroll = self
                        .detail_scroll
                        .saturating_add(3)
                        .min(self.opened_lines.len().saturating_sub(1))
                }
                MouseEventKind::ScrollUp => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(3)
                }
                _ => (),
            }
            return true;
        }
        if self.list_bounds.contains(point) {
            match event.kind {
                MouseEventKind::ScrollDown => self.move_by(1),
                MouseEventKind::ScrollUp => self.move_by(-1),
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some((_, index)) =
                        self.hits.iter().find(|(area, _)| area.contains(point))
                    {
                        if self.sources {
                            self.source_selected = *index;
                        } else {
                            self.selected = *index;
                            self.open_selected();
                        }
                    }
                }
                _ => (),
            }
            return true;
        }
        false
    }
}

#[cfg(test)]
#[path = "my_work_tests.rs"]
mod tests;
