//! Reviewed cycle batches retain their original input, source and request.
use super::*;
use workdeck_pm::{CycleCarryoverPlan, CycleCarryoverRequest};

#[derive(Debug)]
pub(super) struct CarryoverDraft {
    from: String,
    expected: SourceToken,
    form: WorkbenchForm,
    plan: Option<CycleCarryoverPlan>,
    request: Option<RequestId>,
    receipt: Option<MutationReceipt>,
    error: Option<PmError>,
    lines: Vec<String>,
    scroll: usize,
    page_rows: usize,
}
impl PlanningWorkspace {
    pub(super) fn showing_carryover(&self) -> bool {
        self.kind == PlanningKind::Cycle && self.carryover_visible && self.carryover.is_some()
    }
    pub(super) fn start_carryover(&mut self) -> Result<()> {
        if self.carryover.is_none() {
            if self.indexed {
                self.select_index_record()?;
            }
            let selected = self
                .selected()
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "Select a cycle for carryover"))?;
            let mut form = WorkbenchForm::new(
                FormKind::Planning,
                "Cycle carryover",
                vec![
                    TextField::new("Destination cycle ID", String::new(), false),
                    TextField::new(
                        "Issue IDs (blank selects unfinished members)",
                        String::new(),
                        true,
                    ),
                ],
            );
            form.help.push(format!(
                "From {} · Ctrl-S previews without writing",
                selected.metadata.id
            ));
            self.carryover = Some(CarryoverDraft {
                from: selected.metadata.id.clone(),
                expected: selected.source.clone(),
                form,
                plan: None,
                request: None,
                receipt: None,
                error: None,
                lines: Vec::new(),
                scroll: 0,
                page_rows: 10,
            });
        }
        self.carryover_visible = true;
        Ok(())
    }
    pub(super) fn carryover_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.carryover_visible = false;
            return;
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('d') {
            self.carryover = None;
            self.carryover_visible = false;
            if let Err(error) = self.refresh() {
                self.error = Some(error);
            }
            return;
        }
        let repository = match self.repository() {
            Ok(repository) => repository.clone(),
            Err(error) => {
                self.carryover.as_mut().unwrap().error = Some(error);
                return;
            }
        };
        let draft = self.carryover.as_mut().unwrap();
        if draft.plan.is_none() {
            match draft.form.key(key) {
                FormAction::Submit => {
                    if let Err(error) = draft.preview(&repository) {
                        draft.error = Some(error);
                    }
                }
                FormAction::Edited => draft.error = None,
                FormAction::Close => self.carryover_visible = false,
            }
            return;
        }
        if !key.modifiers.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Char('e') => {
                if draft.request.is_some() {
                    draft.error = Some(PmError::new(
                        ErrorCode::IdempotencyConflict,
                        "This carryover was attempted; x retries its exact request. Ctrl-D explicitly discards the retained intent.",
                    ));
                } else {
                    draft.plan = None;
                    draft.error = None;
                }
            }
            KeyCode::Char('x') => {
                let plan = draft.plan.as_ref().unwrap();
                let request = draft.request.get_or_insert_with(RequestId::new);
                match repository.apply_cycle_carryover(&plan.request, &plan.fingerprint, request) {
                    Ok(receipt) => {
                        draft.receipt = Some(receipt.clone());
                        draft.error = None;
                        self.last_receipt = Some(receipt);
                        if let Err(error) = self.refresh() {
                            self.carryover.as_mut().unwrap().error = Some(error);
                        }
                    }
                    Err(error) => draft.error = Some(error),
                }
            }
            KeyCode::PageDown => {
                draft.scroll = draft
                    .scroll
                    .saturating_add(draft.page_rows)
                    .min(draft.lines.len().saturating_sub(1))
            }
            KeyCode::PageUp => draft.scroll = draft.scroll.saturating_sub(draft.page_rows),
            KeyCode::Down | KeyCode::Char('j') => {
                draft.scroll = draft
                    .scroll
                    .saturating_add(1)
                    .min(draft.lines.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::Char('k') => draft.scroll = draft.scroll.saturating_sub(1),
            KeyCode::Home => draft.scroll = 0,
            KeyCode::End => draft.scroll = draft.lines.len().saturating_sub(draft.page_rows),
            _ => {}
        }
    }
}
impl CarryoverDraft {
    fn preview(&mut self, repository: &Repository) -> Result<()> {
        let input = CycleCarryoverRequest {
            from: self.from.clone(),
            to: self.form.fields[0].value.trim().into(),
            issues: self.form.fields[1]
                .value
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<Vec<_>>>()?,
        };
        let plan = repository.preview_cycle_carryover(&input)?;
        if plan.from_source != self.expected {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Source cycle changed after this form opened; discard and inspect a fresh cycle before previewing",
            ));
        }
        self.lines = vec![
            format!("{} -> {}", plan.request.from, plan.request.to),
            format!(
                "Move {} unfinished issues · Keep {} excluded",
                plan.issues.len(),
                plan.excluded.len()
            ),
            "Cycle membership only; issue status and cycle state stay unchanged.".into(),
        ];
        for issue in &plan.issues {
            self.lines
                .push(format!("Move: {} · {}", issue.title, issue.id));
        }
        for excluded in &plan.excluded {
            self.lines.push(format!(
                "Keep ({}): {} · {}",
                excluded.reason, excluded.issue.title, excluded.issue.id
            ));
        }
        self.plan = Some(plan);
        self.error = None;
        self.scroll = 0;
        Ok(())
    }
    pub(super) fn paste(&mut self, text: &str) {
        if self.plan.is_none() {
            self.form.fields[self.form.selected].insert(text);
        }
    }
    pub(super) fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let rows = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(4),
        ])
        .split(area);
        if self.plan.is_none() {
            super::super::shell_view::render_form(&self.form, rows[0], buffer, theme);
        } else {
            self.page_rows = usize::from(rows[0].height.saturating_sub(2)).max(1);
            let text = self
                .lines
                .iter()
                .skip(self.scroll)
                .take(self.page_rows)
                .map(|line| sanitize_terminal_line(line))
                .collect::<Vec<_>>()
                .join("\n");
            Paragraph::new(text)
                .style(style)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(if self.receipt.is_some() {
                            "Carryover saved"
                        } else {
                            "Review cycle carryover"
                        }),
                )
                .render(rows[0], buffer);
        }
        let controls = if self
            .error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::RecoveryRequired)
        {
            "Run workdeck operation recover, then x retries this request"
        } else if self.plan.is_some() {
            "x apply/retry exact plan · e edit before attempt · PgUp/PgDn scroll"
        } else {
            "Tab field · Ctrl-S preview · Ctrl-U clear"
        };
        let status =
            Layout::vertical([Constraint::Length(2), Constraint::Length(1)]).split(rows[1]);
        let request = self
            .request
            .as_ref()
            .map(|id| format!("Request {id}"))
            .unwrap_or_default();
        let error = self
            .error
            .as_ref()
            .map(|error| sanitize_terminal_line(&error.message))
            .unwrap_or_default();
        Paragraph::new(error)
            .style(style)
            .wrap(Wrap { trim: false })
            .render(status[0], buffer);
        Paragraph::new(request)
            .style(style)
            .render(status[1], buffer);
        Paragraph::new(format!(
            "{controls}\nEsc retain · Ctrl-D discard intent · F2 Review · F12 Cycles"
        ))
        .style(style)
        .wrap(Wrap { trim: false })
        .render(rows[2], buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        tempfile::TempDir,
        Repository,
        PlanningWorkspace,
        workdeck_pm::IssueRecord,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let repo = Repository::init(directory.path(), "WD").unwrap();
        for (id, name) in [("current", "A Current"), ("next", "B Next")] {
            repo.create_planning(
                PlanningKind::Cycle,
                &CreatePlanning {
                    id: Some(id.into()),
                    ..CreatePlanning::new(name)
                },
                &RequestId::new(),
            )
            .unwrap();
        }
        let issue = add(&repo, "Original");
        let mut workspace = PlanningWorkspace::new(Some(repo.clone()));
        workspace.open(PlanningKind::Cycle);
        workspace.start_carryover().unwrap();
        workspace.carryover.as_mut().unwrap().paste("next");
        (directory, repo, workspace, issue)
    }
    fn add(repo: &Repository, title: &str) -> workdeck_pm::IssueRecord {
        serde_json::from_value(
            repo.create_issue(
                &workdeck_pm::CreateIssue {
                    title: title.into(),
                    body: "".into(),
                    fields: BTreeMap::from([("cycle".into(), json!("current"))]),
                },
                &RequestId::new(),
            )
            .unwrap()
            .result,
        )
        .unwrap()
    }
    #[test]
    fn stale_carryover_keeps_original_preview_and_request_and_exposes_reset_controls() {
        let (_directory, repo, mut workspace, issue) = fixture();
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let fingerprint = workspace
            .carryover
            .as_ref()
            .unwrap()
            .plan
            .as_ref()
            .unwrap()
            .fingerprint
            .clone();
        add(&repo, "Added after review");
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let request = workspace
            .carryover
            .as_ref()
            .unwrap()
            .request
            .clone()
            .unwrap();
        assert_eq!(
            workspace
                .carryover
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .code,
            ErrorCode::StaleSource
        );
        assert_eq!(
            repo.show_issue(issue.metadata.id.as_str()).unwrap().source,
            issue.source
        );
        for width in [62, 160] {
            let area = Rect::new(0, 0, width, 30);
            let mut buffer = Buffer::empty(area);
            workspace.render(area, &mut buffer, &crate::resolve_theme(None, None, &[]));
            let text = buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                text.contains("Ctrl-D"),
                "stale recovery controls disappeared at width {width}: {text}"
            );
            assert!(text.contains("preview changed"));
        }
        workspace.refresh().unwrap();
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            workspace.carryover.as_ref().unwrap().request.as_ref(),
            Some(&request)
        );
        assert_eq!(
            workspace
                .carryover
                .as_ref()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .fingerprint,
            fingerprint
        );
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(
            workspace
                .carryover
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .code,
            ErrorCode::IdempotencyConflict
        );
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(workspace.carryover.is_none());
        workspace.start_carryover().unwrap();
        workspace.carryover.as_mut().unwrap().paste("next");
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(
            workspace
                .carryover
                .as_ref()
                .unwrap()
                .plan
                .as_ref()
                .unwrap()
                .issues
                .len(),
            2
        );
        assert!(workspace.carryover.as_ref().unwrap().request.is_none());
    }

    #[test]
    fn interrupted_carryover_keeps_recovery_guidance_and_replays_after_recovery() {
        let (_directory, repo, mut workspace, issue) = fixture();
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let draft = workspace.carryover.as_mut().unwrap();
        let plan = draft.plan.as_ref().unwrap();
        let request = RequestId::new();
        assert!(
            repo.apply_cycle_carryover_with_faults(
                &plan.request,
                &plan.fingerprint,
                &request,
                |point| {
                    if point == workdeck_pm::transactions::FaultPoint::AfterJournal {
                        Err(PmError::new(ErrorCode::Io, "injected interruption"))
                    } else {
                        Ok(())
                    }
                }
            )
            .is_err()
        );
        draft.request = Some(request.clone());
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            workspace
                .carryover
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .code,
            ErrorCode::RecoveryRequired
        );
        let area = Rect::new(0, 0, 62, 30);
        let mut buffer = Buffer::empty(area);
        workspace.render(area, &mut buffer, &crate::resolve_theme(None, None, &[]));
        let text = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("workdeck operation recover"));
        assert!(text.contains("Ctrl-D"));
        repo.recover_operations().unwrap();
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            workspace
                .carryover
                .as_ref()
                .unwrap()
                .receipt
                .as_ref()
                .unwrap()
                .request_id,
            request
        );
        assert_eq!(
            repo.show_issue(issue.metadata.id.as_str())
                .unwrap()
                .metadata
                .cycle
                .as_deref(),
            Some("next")
        );
    }
    #[test]
    fn source_cycle_edit_does_not_rebase_the_open_carryover_form() {
        let (_directory, repo, mut workspace, _) = fixture();
        let original = workspace.carryover.as_ref().unwrap().expected.clone();
        repo.mutate_planning(
            PlanningKind::Cycle,
            "current",
            Some(&original),
            &PlanningMutation::Update {
                fields: BTreeMap::from([("name".into(), json!("Changed cycle"))]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
        workspace.carryover_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let draft = workspace.carryover.as_ref().unwrap();
        assert_eq!(draft.error.as_ref().unwrap().code, ErrorCode::StaleSource);
        assert!(draft.plan.is_none());
        assert!(draft.request.is_none());
        assert_eq!(draft.expected, original);
        assert_eq!(draft.form.fields[0].value, "next");
    }
}
