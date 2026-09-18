use super::source_actions::{ReviewedSourceAction, SourceActionOutcome, SourceActions};
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Paragraph, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;
use workdeck_pm::sources::ProposalPlan;

fn safe(text: &str) -> String {
    text.lines()
        .map(sanitize_terminal_line)
        .collect::<Vec<_>>()
        .join("\n")
}
fn proposal_lines(plan: &ProposalPlan, page: usize) -> Vec<String> {
    const PAGE: usize = 64;
    let pages = (plan.changed.len() + plan.proposal_changed.len())
        .max(1)
        .div_ceil(PAGE);
    let page = page.min(pages - 1);
    let mut lines = vec![
        "Reviewed proposal plan · x explicitly publishes this retained plan".into(),
        format!("{} · {}", plan.reference, plan.title),
        format!(
            "Repository {} · application base {}",
            plan.repository, plan.application_base
        ),
        format!(
            "Accepted {:?} · commit {}",
            plan.accepted.role,
            plan.accepted
                .commit
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "none".into())
        ),
        format!(
            "Working source {} · configuration {}",
            plan.source.content, plan.config
        ),
        format!("Reviewed Git/remote binding {}", plan.binding),
        format!("Plan fingerprint {}", plan.fingerprint),
        format!(
            "{} planning changes vs accepted; {} vs proposal · page {}/{} · [/] pages",
            plan.changed.len(),
            plan.proposal_changed.len(),
            page + 1,
            pages
        ),
    ];
    for (basis, change) in plan
        .changed
        .iter()
        .map(|change| ("accepted", change))
        .chain(
            plan.proposal_changed
                .iter()
                .map(|change| ("proposal", change)),
        )
        .skip(page * PAGE)
        .take(PAGE)
    {
        lines.push(format!(
            "{} · compared with {basis}\n  {} → {}",
            change.path.display(),
            change
                .before
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "absent".into()),
            change
                .after
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "absent".into())
        ));
    }
    lines
}
impl SourceActions {
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let parts = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(4),
        ])
        .split(area);
        Paragraph::new("Source operations · explicit remote actions\nOpening Sources or this view does not fetch or publish.")
            .style(style.add_modifier(Modifier::BOLD)).render(parts[0], buffer);
        if let Some(form) = self.form() {
            super::shell_view::render_form(form, parts[1], buffer, theme);
        } else {
            let mut lines = Vec::new();
            if let Some(outcome) = &self.outcome {
                match outcome {
                    SourceActionOutcome::Refresh(outcome) => {
                        lines.push(format!(
                            "Source refresh recorded · operation {} · request {} · replayed {}",
                            outcome.operation_id, outcome.request_id, outcome.replayed
                        ));
                        for observation in &outcome.observations {
                            lines.push(format!(
                                "Remote {} · {} · commit {} · observed {}",
                                observation.remote,
                                observation.reference,
                                observation
                                    .commit
                                    .as_ref()
                                    .map(ToString::to_string)
                                    .unwrap_or_else(|| "absent".into()),
                                observation.observed_at
                            ));
                        }
                        lines.push("Immutable source slots and opened citations were retained; r explicitly captures a new reader view.".into());
                    }
                    SourceActionOutcome::Proposal(outcome) => {
                        lines.push(format!(
                            "Proposal {:?} · request {} · replayed {}",
                            outcome.state, outcome.request_id, outcome.replayed
                        ));
                        lines.push(format!(
                            "Candidate {} · original plan {}",
                            outcome.candidate, outcome.plan.fingerprint
                        ));
                        if let Some(current) = &outcome.current {
                            lines.push(format!(
                                "Current observation {:?} · {:?} · {}",
                                current.identity.role, current.freshness, current.observed_at
                            ));
                        }
                        lines.push(outcome.reason_codes.join(" · "));
                        lines.push(
                            "Proposal publication does not promote accepted planning state.".into(),
                        );
                    }
                }
                lines.push(String::new());
            }
            match &self.review {
                Some(ReviewedSourceAction::Refresh { sync, input, source, shared, errors }) => {
                    lines.push(format!("Reviewed {} plan · x explicitly executes", if *sync { "Sync" } else { "Fetch" }));
                    lines.push(format!("Remote {} · accepted {} · coordination {}", shared.remote, shared.accepted_ref, shared.coordination_ref));
                    lines.push(format!("Repository {} · observed {}", source.identity.repository, source.observed_at));
                    lines.push(format!("Exact configuration {}", input.expected_config));
                    lines.push(format!("Reviewed Git/remote binding {}", input.expected_binding));
                    for error in errors { lines.push(format!("Cached source unavailable · {:?}: {}", error.code, error.message)); }
                    lines.push(if *sync { "Sync refreshes the source cache and ignored materialized planning views; the developer index and worktree remain separate." } else { "Fetch refreshes explicit planning source observations and their ignored cache; it does not check out the developer branch." }.into());
                },
                Some(ReviewedSourceAction::Proposal(plan)) => lines.extend(proposal_lines(plan, self.page)),
                None if self.outcome.is_none() => lines.push("f reviews Fetch · s reviews Sync · n previews a Proposal\nt inspects an original proposal request for status or explicit resume.\n\nNo plan is selected. Publication requires a separate x action after inspection.".into()),
                None => {},
            }
            if let Some(request) = &self.request {
                lines.push(format!("Retained original request {request}"));
            }
            Paragraph::new(safe(&lines.join("\n")))
                .style(style)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0))
                .render(parts[1], buffer);
        }
        let mut footer = Vec::new();
        if self.busy() {
            footer.push(
                "Source operation pending; exit and suspension are deferred until join.".into(),
            );
        }
        if let Some(error) = &self.error {
            footer.push(format!("{:?}: {}", error.code, error.message));
        }
        footer.push(if self.form().is_some() { "Tab field · Ctrl-S inspect · Esc retain draft · Ctrl-D discard".into() }
            else { "f Fetch plan · s Sync plan · n Proposal preview · x explicit execute\nv proposal status · u resume · t request · Ctrl-D new intent · Esc Sources\nPgUp/PgDn scroll · [/] change pages · F2 Review".into() });
        Paragraph::new(safe(&footer.join("\n")))
            .style(style)
            .wrap(Wrap { trim: false })
            .render(parts[2], buffer);
    }
}
