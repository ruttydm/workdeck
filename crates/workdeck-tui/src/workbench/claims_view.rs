use super::claims_workspace::ClaimsWorkspace;
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Paragraph, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;

fn safe(text: &str) -> String {
    text.lines()
        .map(sanitize_terminal_line)
        .collect::<Vec<_>>()
        .join("\n")
}

impl ClaimsWorkspace {
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let layout = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(4),
        ])
        .split(area);
        Paragraph::new("Work claims · explicit coordination\nHistorical receipt and current ownership are separate.")
            .style(style.add_modifier(Modifier::BOLD)).render(layout[0], buffer);
        if let Some(form) = self.form() {
            super::shell_view::render_form(form, layout[1], buffer, theme);
        } else {
            let mut lines = Vec::new();
            if let Some(state) = self.state() {
                if let Some(contract) = &state.contract {
                    let source = &contract.accepted_source;
                    lines.push(format!(
                        "Issue {} · {:?} · revision {}",
                        contract.issue,
                        source.role,
                        contract.issue_source.revision.get()
                    ));
                    lines.push(format!(
                        "Ref {} · commit {}",
                        source
                            .ref_name
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "local".into()),
                        source
                            .commit
                            .as_ref()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| "none".into())
                    ));
                    lines.push(format!("Contract requirements {}", contract.requirements));
                } else {
                    lines.push("Inspect an exact local or accepted issue contract first. No claim is acquired by reading this view.".into());
                }
                if let Some(binding) = &state.binding {
                    lines.push(format!("Inspected publication binding {binding}"));
                }
                if let Some(status) = &state.status {
                    let metadata = &status.claim.metadata;
                    let assessment = &status.assessment;
                    if state.error.is_some() {
                        lines.push("Prior claim observation retained; its assessment is not a refreshed permission to continue.".into());
                    }
                    lines.push(format!(
                        "Authored {:?} · assessment {:?} · guarantee {:?}",
                        metadata.state, assessment.disposition, assessment.guarantee
                    ));
                    lines.push(format!(
                        "Actor {} · generation {} · token {}",
                        metadata.actor, metadata.generation, metadata.token
                    ));
                    lines.push(format!(
                        "Expires {} · assessed {}",
                        metadata.expires_at, assessment.assessed_at
                    ));
                    lines.push(format!(
                        "May continue: {} · recovery required: {}",
                        assessment.may_continue, assessment.recovery_required
                    ));
                    lines.push(format!(
                        "Source {:?} · {:?} · observed {}",
                        status.source.identity.role,
                        status.source.freshness,
                        status.source.observed_at
                    ));
                    lines.push(assessment.reason_codes.join(" · "));
                } else {
                    lines.push("Current ownership: unknown or no captured claim. This view does not infer permission to continue.".into());
                }
                if let Some(outcome) = &state.outcome {
                    lines.push(format!(
                        "Historical request {} · receipt {}",
                        outcome.request_id,
                        outcome
                            .receipt
                            .as_ref()
                            .map(|receipt| receipt.operation_id.to_string())
                            .unwrap_or_else(|| "unavailable".into())
                    ));
                    lines.push(format!(
                        "Publication {} · requested token current: {} · may continue: {}",
                        outcome
                            .publication
                            .as_ref()
                            .map(|publication| format!("{:?}", publication.state))
                            .unwrap_or_else(|| "local only".into()),
                        outcome.requested_token_current,
                        outcome.may_continue
                    ));
                    lines.push(outcome.reason_codes.join(" · "));
                }
                if let Some(completion) = &state.completion {
                    lines.push(format!(
                        "{} · receipt {} · request {}",
                        if state.completion_verified {
                            "Authenticated claimed completion recorded"
                        } else {
                            "Claimed completion recorded"
                        },
                        completion.completion.operation_id,
                        completion.completion.request_id
                    ));
                    if state.completion_verified {
                        lines.push("Authenticated checks and gate proof are retained in the receipt; no separate release was requested.".into());
                    } else {
                        lines.push(format!(
                            "Separate release recorded: {} · receipt {}",
                            completion.release_recorded,
                            completion
                                .release
                                .as_ref()
                                .map(|receipt| receipt.operation_id.to_string())
                                .unwrap_or_else(|| "unavailable".into())
                        ));
                    }
                    if let Some(publication) = &completion.release_publication {
                        lines.push(format!(
                            "Release publication {:?} · requested token current {}",
                            publication
                                .publication
                                .as_ref()
                                .map(|outcome| outcome.state),
                            publication.requested_token_current
                        ));
                    }
                    if let Some(error) = &completion.release_error {
                        lines.push(format!(
                            "Release {:?}: {} · completion remains recorded",
                            error.code, error.message
                        ));
                    }
                }
                if let Some(notice) = &state.notice {
                    lines.push(notice.clone());
                }
                if let Some(request) = state
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.request.as_ref())
                {
                    lines.push(format!("Retained original request: {request}"));
                }
                if let Some(release) = state
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.release_request.as_ref())
                {
                    lines.push(format!("Retained separate release request: {release}"));
                }
            } else {
                lines.push("Select an issue in Issues or Sources, then open Claims. Initialization and publication are explicit actions.".into());
            }
            Paragraph::new(safe(&lines.join("\n")))
                .style(style)
                .wrap(Wrap { trim: false })
                .scroll((self.state().map_or(0, |state| state.scroll), 0))
                .render(layout[1], buffer);
        }
        let mut footer = Vec::new();
        if self.busy() {
            footer.push(if self.signal.interruption_requested() { "Publication pending · exit requested and deferred until join; outcome is not canceled." } else { "Publication pending · tabs remain usable; exit and suspension wait for the owned worker." }.to_owned());
        }
        if let Some(state) = self.state()
            && let Some(error) = &state.error
        {
            footer.push(format!("{:?}: {}", error.code, error.message));
        }
        footer.push(if self.form().is_some() { "Tab field · Ctrl-S explicit publish · Esc retain · Ctrl-D discard".into() }
            else { "n acquire · u renew · v revalidate · d release · e complete · g verified\nc cancel · s supersede · x recover · r status · t retry\nPgUp/PgDn scroll · 1–6 sections · F2 Review".into() });
        Paragraph::new(safe(&footer.join("\n")))
            .style(style)
            .wrap(Wrap { trim: false })
            .render(layout[2], buffer);
    }
}
