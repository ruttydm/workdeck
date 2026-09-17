use super::context_workspace::{ContextWorkspace, Row, RowTarget, Section};
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;
use workdeck_pm::*;

impl ContextWorkspace {
    pub fn rows(&self) -> Vec<Row> {
        let Some(state) = self.state() else {
            return Vec::new();
        };
        if state.section == Section::Next && state.ready_visible {
            let Some(ready) = &state.ready else {
                return Vec::new();
            };
            let mut rows = vec![Row {
                id: "ready:summary".into(),
                title: format!(
                    "Ready work · {} eligible / {} excluded",
                    ready.eligible, ready.excluded
                ),
                body: format!(
                    "{}\nInspected selection: {}\n{} candidates. Eligibility and priority order come from the shared engine. Enter inspects a task; no work is started.\n] next page · r first page",
                    if ready.eligible == 0 {
                        "No eligible work."
                    } else {
                        "Ready work is available."
                    },
                    ready.fingerprint,
                    ready.total
                ),
                target: None,
            }];
            rows.extend(ready.candidates.iter().map(|candidate| Row {
                id: format!("ready:{}", candidate.issue),
                title: format!(
                    "{} · {}",
                    if candidate.eligible {
                        "eligible"
                    } else {
                        "excluded"
                    },
                    candidate.title
                ),
                body: format!(
                        "{}\n{}\n{}\n{}",
                        candidate.issue,
                        candidate.title,
                        candidate.reason_codes.join(" · "),
                        candidate
                            .conditions
                            .iter()
                            .map(|condition| format!(
                                "{:?}: {}",
                                condition.state, condition.message
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    ),
                target: Some(RowTarget::Ready(candidate.clone())),
            }));
            return rows;
        }
        let mut rows = if state.section == Section::Context {
            state.review.rows()
        } else {
            Vec::new()
        };
        if state.section == Section::Next {
            rows.push(Row { id: "ready:list".into(), title: "Browse ready work".into(), body: "Enter shows repository-wide eligibility and exclusion reasons. Suggestions below concern this inspected task.".into(), target: Some(RowTarget::ReadyList) });
        }
        if let Some(packet) = &state.packet {
            for section in &packet.sections {
                let visible = match state.section {
                    Section::Context => !matches!(
                        section.kind,
                        ContextSectionKind::Actions
                            | ContextSectionKind::Questions
                            | ContextSectionKind::Handoffs
                    ),
                    Section::Next => section.kind == ContextSectionKind::Actions,
                    Section::Questions => section.kind == ContextSectionKind::Questions,
                    Section::Handoffs => section.kind == ContextSectionKind::Handoffs,
                    Section::Checks | Section::Sources | Section::Claims => false,
                };
                if !visible {
                    continue;
                }
                for entry in &section.entries {
                    let row = content_row(&entry.content);
                    let key = row.id.clone();
                    rows.push(row);
                    if state.section != Section::Next {
                        for (index, citation) in entry.citations.iter().enumerate() {
                            rows.push(Row { id: format!("{key}:citation:{index}"), title: format!("↳ {}", citation_label(&citation.target)),
                                body: format!("{}\nInspected content: {}\nEnter opens this source after checking the captured context and content.", citation_label(&citation.target), citation.source.as_ref().map(ToString::to_string).unwrap_or_else(|| "unavailable".into())),
                                target: Some(RowTarget::Citation(citation.clone())) });
                        }
                    }
                }
                if section.omitted != 0 {
                    rows.push(Row { id: format!("omitted:{:?}", section.kind), title: format!("{} {:?} entries omitted", section.omitted, section.kind),
                        body: "This packet is incomplete by design. Press + for a larger fresh packet; existing draft preconditions remain unchanged.".into(), target: None });
                }
            }
        }
        rows
    }
    pub fn render(
        &mut self,
        area: Rect,
        buffer: &mut Buffer,
        theme: &AppTheme,
        notice: Option<&str>,
    ) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(4),
        ])
        .split(area);
        let title = self
            .state()
            .and_then(|state| state.packet.as_ref())
            .and_then(|packet| {
                packet
                    .sections
                    .iter()
                    .flat_map(|section| &section.entries)
                    .find_map(|entry| match &entry.content {
                        ContextContent::Summary { title, .. } => Some(title.as_str()),
                        _ => None,
                    })
            })
            .unwrap_or("Select ready work or refresh task context");
        let section = self.state().map(|state| state.section).unwrap_or_default();
        let title = match section {
            Section::Sources => "Planning source inspection",
            Section::Claims => "Work claim inspection",
            _ => title,
        };
        Paragraph::new(sanitize_terminal_line(&format!("Task context · {title}")))
            .style(style.add_modifier(Modifier::BOLD))
            .render(layout[0], buffer);
        Paragraph::new(if area.width < 110 {
            "1 Context 2 Next 3 Qs 4 Handoff 5 Checks 6 Sources 7 Claims".into()
        } else { format!("1 Context · 2 Next · 3 Questions · 4 Handoffs · 5 Checks · 6 Sources · 7 Claims [{section:?}]") })
        .style(style)
        .render(layout[1], buffer);
        if section == Section::Sources && self.source_actions.visible {
            self.source_actions.render(
                Rect::new(
                    layout[2].x,
                    layout[2].y,
                    layout[2].width,
                    layout[2].height.saturating_add(layout[3].height),
                ),
                buffer,
                theme,
            );
            return;
        }
        if section == Section::Sources {
            self.sources.render(
                Rect::new(
                    layout[2].x,
                    layout[2].y,
                    layout[2].width,
                    layout[2].height.saturating_add(layout[3].height),
                ),
                buffer,
                theme,
            );
            return;
        }
        if section == Section::Claims {
            self.claims.render(
                Rect::new(
                    layout[2].x,
                    layout[2].y,
                    layout[2].width,
                    layout[2].height.saturating_add(layout[3].height),
                ),
                buffer,
                theme,
            );
            return;
        }
        if section == Section::Checks {
            self.checks.render(
                Rect::new(
                    layout[2].x,
                    layout[2].y,
                    layout[2].width,
                    layout[2].height.saturating_add(layout[3].height),
                ),
                buffer,
                theme,
            );
            return;
        }
        if let Some(form) = self.form() {
            super::shell_view::render_form(form, layout[2], buffer, theme);
        } else {
            let rows = self.rows();
            let state = self.state_mut();
            let index = state.section.index();
            let selected = rows
                .iter()
                .position(|row| Some(&row.id) == state.selected[index].as_ref())
                .or_else(|| (!rows.is_empty()).then_some(0));
            let columns = if area.width >= 96 {
                Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                    .split(layout[2])
            } else {
                Layout::vertical([Constraint::Length(6), Constraint::Min(0)]).split(layout[2])
            };
            let mut list_state = ListState::default()
                .with_selected(selected)
                .with_offset(state.offset[index]);
            StatefulWidget::render(
                List::new(
                    rows.iter()
                        .map(|row| ListItem::new(sanitize_terminal_line(&row.title))),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" {section:?} ")),
                )
                .highlight_symbol("› ")
                .highlight_style(style.add_modifier(Modifier::BOLD)),
                columns[0],
                buffer,
                &mut list_state,
            );
            state.offset[index] = list_state.offset();
            let text = selected.and_then(|index| rows.get(index)).map(|row| row.body.clone()).unwrap_or_else(|| match section {
                Section::Questions => "No questions included in this packet. n creates a question for the inspected issue. Check packet omissions before treating this as exhaustive.".into(),
                Section::Handoffs => "No handoffs included in this packet. n records declared continuity for the inspected issue.".into(),
                _ => "No context available. Press r to refresh; F2 returns to Review.".into(),
            });
            Paragraph::new(safe_text(&text))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Inspected details "),
                )
                .style(style)
                .wrap(Wrap { trim: false })
                .scroll((state.scroll[index], 0))
                .render(columns[1], buffer);
        }
        let mut footer = Vec::new();
        if let Some(notice) = notice {
            footer.push(notice.to_owned());
        }
        if let Some(state) = self.state() {
            if let Some(error) = &state.error {
                footer.push(format!(
                    "{} · {}",
                    error.message,
                    error
                        .hint
                        .as_deref()
                        .unwrap_or("Retained source and draft were not rebased.")
                ));
            }
            if let Some(receipt) = &state.receipt {
                footer.push(format!("Saved operation {}", receipt.operation_id));
            }
            if let Some(packet) = &state.packet {
                footer.push(format!(
                    "{} / {} bytes · {} omitted · evidence time {}",
                    packet.budget.used_bytes,
                    packet.budget.limit_bytes,
                    packet.budget.omitted_entries,
                    if packet.as_of.is_none() {
                        "unknown (no as-of time)"
                    } else {
                        "see each record"
                    }
                ));
            }
        }
        footer.push(if self.state().is_some_and(|s| s.review.visible) { "Tab field · Ctrl-S inspect · Esc retain draft · Ctrl-D discard".into() } else if self.form().is_some() { "Tab field · Ctrl-S save · Esc retain draft · Ctrl-D discard".into() } else {
            match section {
                Section::Questions => "n question · a answer · s supersede · Enter inspect · r refresh · +/- budget · Esc back".into(),
                Section::Handoffs => "n handoff · r refresh · +/- budget · PgUp/PgDn scroll · Esc back".into(),
                Section::Next => "Enter inspect action/task · w ready work · ] next page · r refresh · Esc back".into(),
                Section::Context => "v authenticate review · Enter source · r refresh · +/- budget · PgUp/PgDn scroll · Esc back".into(),
                Section::Checks => "p plan · x explicit run · v results · Ctrl-C cancel".into(),
                Section::Sources => "l working tree · a accepted · p proposal · c coordination · r capture".into(),
                Section::Claims => "n acquire · u renew · v revalidate · d release · r inspect current".into(),
            }
        });
        Paragraph::new(safe_text(&footer.join("\n")))
            .style(style)
            .wrap(Wrap { trim: false })
            .render(layout[3], buffer);
    }
}
fn safe_text(text: &str) -> String {
    text.lines()
        .map(sanitize_terminal_line)
        .collect::<Vec<_>>()
        .join("\n")
}
fn citation_label(target: &ContextTarget) -> String {
    match target {
        ContextTarget::Issue { id } => format!("Issue {id}"),
        ContextTarget::Feature { id } => format!("Feature {id}"),
        ContextTarget::Question { id } => format!("Question {id}"),
        ContextTarget::Handoff { issue, id } => format!("Handoff {id} · {issue}"),
        ContextTarget::Evidence { id } => format!("Evidence {id}"),
        ContextTarget::PlanningSource { path } => format!("Planning source {}", path.display()),
        ContextTarget::WorktreeSource { link } => format!(
            "{}{}",
            link.path,
            link.line.map(|line| format!(":{line}")).unwrap_or_default()
        ),
    }
}
fn content_row(content: &ContextContent) -> Row {
    let (id, title, body, target) = match content {
        ContextContent::ContractReview {
            summary,
            selected_revision,
        } => (
            format!("contract-review:{}", summary.review.id),
            format!(
                "Contract review · {:?} · {}",
                summary.state, summary.review.id
            ),
            format!(
                "Historical signed contract review\nAssessment: {:?}\nAssessed HEAD: {}\nReasons: {}\nReviewed commit: {}\nReviewed contract: {}\nHistorical reviewers: {}\nCurrent authenticated reviewers: {}\nExpires: {}\nImported by: {}\n\nTask context compares live planning contracts and declared evaluator files with assessed HEAD. Other application files are outside this assessment. A matching historical review requires independent current policy pins before authentication. This does not establish passing checks or completion. The citation opens the retained proof.",
                summary.state,
                selected_revision
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "unavailable".into()),
                summary.reason_codes.join(" · "),
                summary.review.admission.approval.head.commit,
                summary.review.admission.approval.head_contract,
                summary.review.admission.reviewers.join(", "),
                if summary.current_reviewers.is_empty() {
                    "none".into()
                } else {
                    summary.current_reviewers.join(", ")
                },
                summary.review.admission.approval.expires_at.to_rfc3339(),
                summary.review.actor
            ),
            None,
        ),
        ContextContent::CheckRun { summary } => (
            format!("check-run:{}", summary.id),
            format!("Local checks · {:?} · {}", summary.state, summary.id),
            format!(
                "Inspected local feedback\nCurrent assessment: {:?}\nHistorical result: {:?}\nReasons: {}\nPlan: {}\nFailures ({} omitted):\n{}\nArtifacts ({} omitted):\n{}\nEnter opens this exact run in Checks; it does not execute a command.",
                summary.state,
                summary.historical_state,
                summary.reason_codes.join(" · "),
                summary.plan,
                summary.omitted_failures,
                summary
                    .failures
                    .iter()
                    .map(|failure| format!("{}: {}", failure.check, failure.diagnostic.message))
                    .collect::<Vec<_>>()
                    .join("\n"),
                summary.omitted_artifacts,
                summary
                    .artifacts
                    .iter()
                    .map(|artifact| format!("{} · {:?}", artifact.id, artifact.availability))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Some(RowTarget::CheckRun(Box::new(summary.clone()))),
        ),
        ContextContent::Document {
            reference,
            reason_code,
        } => (
            format!("document:{reference}"),
            format!("Document · {reference}"),
            format!(
                "{reference}\n\n{reason_code}\nLinked document reference; its contents have not been fetched. The citation identifies the issue that declares this link."
            ),
            None,
        ),
        ContextContent::Requirement {
            id,
            description,
            checked_declaration,
        } => (
            format!("requirement:{id}"),
            format!("Requirement {id}"),
            format!(
                "{description}\n\nChecked declaration: {checked_declaration}\nA checked declaration is not execution proof."
            ),
            None,
        ),
        ContextContent::Summary {
            title,
            status,
            body,
            manual_acceptance,
            imported_completion,
        } => (
            "summary".into(),
            title.clone(),
            format!(
                "{title}\nStatus: {status}\n\n{body}\n\nManual acceptance: {}\nImported completion: {}",
                if manual_acceptance.is_some() {
                    "explicit recorded attribution"
                } else {
                    "none"
                },
                if imported_completion.is_some() {
                    "historical provenance; not new verification"
                } else {
                    "none"
                }
            ),
            None,
        ),
        ContextContent::Condition { condition } => (
            format!(
                "condition:{:?}:{}:{:?}",
                condition.kind, condition.reason_code, condition.subject
            ),
            format!("{:?} · {}", condition.state, condition.message),
            format!(
                "{:?}\n{}\nReason: {}\nBasis: {}\nPath: {:?}",
                condition.state,
                condition.message,
                condition.reason_code,
                condition.basis,
                condition.path
            ),
            None,
        ),
        ContextContent::Action { action } => (
            format!("action:{:?}:{:?}", action.kind, action.target),
            format!(
                "{:?}{}",
                action.kind,
                if action.available {
                    ""
                } else {
                    " · unavailable"
                }
            ),
            format!(
                "{}\nReason: {}\nTarget: {}\nContext: {}\nRequirements: {}\nGraph: {}",
                action.explanation,
                action.reason_code,
                citation_label(&action.target),
                action.preconditions.context,
                action.preconditions.requirements,
                action.preconditions.graph
            ),
            Some(RowTarget::Action(action.clone())),
        ),
        ContextContent::Feature { record } => (
            format!("feature:{}", record.metadata.id),
            format!("Feature · {}", record.metadata.name),
            format!(
                "{}\nDecision {:?} · Maturity {:?} · Availability {:?}\nDeclared capability\n\n{}",
                record.metadata.id,
                record.metadata.decision,
                record.metadata.maturity,
                record.metadata.availability,
                record.body
            ),
            None,
        ),
        ContextContent::Verification {
            policy,
            execution_available,
            reason_code,
        } => (
            "verification".into(),
            "Verification requirements".into(),
            format!(
                "Policy: {policy:?}\nExecution available: {execution_available}\n{reason_code}\nDeclared references do not verify execution."
            ),
            None,
        ),
        ContextContent::Source {
            link,
            excerpt,
            reason_code,
        } => (
            format!("source:{}:{:?}", link.path, link.line),
            format!("Source · {}", link.path),
            format!(
                "{}\n{}\n{}",
                link.path,
                reason_code.as_deref().unwrap_or(""),
                excerpt.as_deref().unwrap_or("No excerpt included")
            ),
            None,
        ),
        ContextContent::Instruction { path, text } => (
            format!("instruction:{}", path.display()),
            format!("Instructions · {}", path.display()),
            text.clone(),
            None,
        ),
        ContextContent::Evidence {
            record,
            freshness,
            reason_codes,
        } => (
            format!("evidence:{}", record.reference.id),
            format!("Declared evidence · {freshness:?}"),
            format!(
                "{}\nFreshness: {freshness:?}\n{}\nDeclared by {}: {}\nSubject: {}\nCheck: {}\nResult: {}\nNo admitted passed or verified result.",
                record.reference.id,
                reason_codes.join("\n"),
                record.reference.declaration.provenance.actor,
                record.reference.declaration.provenance.reason,
                record.reference.declaration.subject.content,
                record.reference.declaration.check.id,
                record.reference.declaration.result.id
            ),
            None,
        ),
        ContextContent::Overlap { overlap } => (
            format!("overlap:{}:{:?}", overlap.issue, overlap.basis),
            format!("Advisory overlap · {}", overlap.issue),
            format!(
                "{:?}\nSource: {}\nAdvisory only; no ownership, allocation, or concurrency guarantee.",
                overlap.basis, overlap.source.content
            ),
            None,
        ),
        ContextContent::Notice {
            reason_code,
            message,
        } => (
            format!("notice:{reason_code}"),
            reason_code.clone(),
            message.clone(),
            None,
        ),
        ContextContent::Question {
            record,
            applicability,
        } => {
            let meta = &record.metadata;
            let mut body = format!(
                "{}\n{:?} · {:?}\n{}\n\nBlocks implementation: {}\n{}",
                meta.id,
                meta.state,
                applicability.freshness,
                record.body,
                applicability.blocks_implementation,
                applicability
                    .stale_reasons
                    .iter()
                    .map(|reason| reason.message.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            if let Some(answer) = &meta.answer {
                body.push_str(&format!(
                    "\n\nAnswer by {} at {}\n{}",
                    answer.actor, answer.answered_at, answer.body
                ));
            }
            if let Some(supersession) = &meta.supersession {
                body.push_str(&format!(
                    "\n\nSuperseded by {}\n{}",
                    supersession.replacement, supersession.reason
                ));
            }
            body.push_str(&format!(
                "\n\nAnswer allowed: {}\nSupersede allowed: {}\nSource: {}",
                applicability.answer.allowed,
                applicability.supersede.allowed,
                record.source.content
            ));
            (
                format!("question:{}", meta.id),
                format!(
                    "{:?} · {:?} · {}",
                    meta.state,
                    applicability.freshness,
                    record.body.lines().next().unwrap_or("Question")
                ),
                body,
                Some(RowTarget::Question(Box::new((
                    record.clone(),
                    applicability.clone(),
                )))),
            )
        }
        ContextContent::Handoff {
            record,
            freshness,
            reason_codes,
        } => {
            let meta = &record.metadata;
            (
                format!("handoff:{}", meta.id),
                format!("{freshness:?} · {} · {}", meta.actor, meta.created_at),
                format!(
                    "Declared handoff {}\nFreshness: {freshness:?}\n{}\n\n{}\n\nAttempted\n{}\n\nUncertainties\n{}\n\nNext steps\n{}\n\nEvidence references (declared)\n{}\nQuestions: {}\nPending reconciliation: {}\nCaptured context: {}",
                    meta.id,
                    reason_codes.join("\n"),
                    record.body,
                    meta.attempted.join("\n"),
                    meta.uncertainties.join("\n"),
                    meta.next_steps.join("\n"),
                    meta.evidence_refs
                        .iter()
                        .map(|item| format!("{} {}", item.id, item.content))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    meta.questions
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                    meta.pending_operations
                        .iter()
                        .map(|item| item.request_id.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    meta.anchor.fingerprint
                ),
                Some(RowTarget::Handoff),
            )
        }
    };
    Row {
        id,
        title,
        body,
        target,
    }
}
