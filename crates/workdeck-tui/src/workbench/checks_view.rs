use super::checks_workspace::*;
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;
use workdeck_pm::*;

impl ChecksWorkspace {
    pub fn rows(&self) -> Vec<CheckRow> {
        let Some(state) = self.state() else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        match state.page {
            ChecksPage::Definitions => {
                if let Some(catalog) = &state.catalog {
                    for profile in &catalog.profiles {
                        let value = &profile.definition;
                        rows.push(CheckRow { id: format!("profile:{}", value.id), title: format!("Profile · {}{}", value.name, if value.archived { " [archived]" } else { "" }),
                        detail: format!("Profile {}\n{}\nChecks: {}\nSource: {}\nContent: {}\np builds an inspected plan; no process starts.", value.id, excerpt(&value.description), value.checks.join(", "), profile.path.display(), profile.content),
                        target: Some(CheckTarget::Profile(value.id.clone())) });
                    }
                    for check in &catalog.checks {
                        let value = &check.definition;
                        rows.push(CheckRow { id: format!("check:{}", value.id), title: format!("Check · {}{}", value.name, if value.archived { " [archived]" } else { "" }),
                        detail: format!("Check {}\n{}\nCommand: {}\nExpectation: {}\nSource: {}\nContent: {}", value.id, excerpt(&value.description), value.command, expectation(&value.expectation), check.path.display(), check.content),
                        target: Some(CheckTarget::Check(value.id.clone())) });
                    }
                    for command in &catalog.commands {
                        let value = &command.definition;
                        let recipe = match &value.recipe {
                            CommandRecipe::Argv { argv } => {
                                format!("Explicit argv · {} tokens", argv.len())
                            }
                            CommandRecipe::Shell { interpreter, .. } => {
                                format!("Explicit shell recipe · {interpreter}")
                            }
                        };
                        rows.push(CheckRow { id: format!("command:{}", value.id), title: format!("Command · {}{}", value.name, if value.archived { " [archived]" } else { "" }),
                        detail: format!("Command {}\n{}\n{}\nWorking directory: {}\nTimeout: {}s\nEffects:\n{}\nParameters: {}\nSource: {}\nContent: {}", value.id, excerpt(&value.description), recipe, value.cwd.display(), value.bounds.timeout_seconds, effects(&value.effects), value.parameters.keys().cloned().collect::<Vec<_>>().join(", "), command.path.display(), command.content),
                        target: Some(CheckTarget::Command(value.id.clone())) });
                    }
                }
            }
            ChecksPage::Plan => {
                if let Some(inspected) = &state.plan {
                    let plan = &inspected.plan;
                    rows.push(CheckRow { id: "plan:summary".into(), title: format!("Inspected plan · {} invocations · {} blockers", plan.invocations.len(), plan.blockers.len()),
                    detail: format!("Local feedback only\nRepository: {}\nPlan: {}\nConfiguration: {}\nIssue: {}\nRequest: {}\n{}\nSelection:\n{}\nBlockers:\n{}\nReview invocations before x executes this exact plan.", plan.repository, plan.fingerprint, plan.config,
                        plan.issue.as_ref().map(|issue| format!("{} · revision {} · {}\nRequirements: {}", issue.id, issue.source.revision.get(), issue.source.content, issue.requirements)).unwrap_or_else(|| "direct command, no issue binding".into()),
                        inspected.request.as_ref().map(ToString::to_string).unwrap_or_else(|| "allocated on explicit run".into()),
                        if plan.blockers.is_empty() { "No planner blockers. Inputs are checked again before execution." } else { "Planner blockers must be resolved before execution." },
                        plan.selection.iter().map(|reason| format!("{}: {}{}", reason.check, reason.reason_code, reason.source.as_ref().map(|source| format!(" ({source})")).unwrap_or_default())).collect::<Vec<_>>().join("\n"),
                        plan.blockers.iter().map(|diagnostic| format!("{}: {}", diagnostic.reason_code, diagnostic.message)).collect::<Vec<_>>().join("\n")), target: None });
                    for invocation in &plan.invocations {
                        let args = invocation
                            .args
                            .iter()
                            .enumerate()
                            .map(|(index, arg)| match arg {
                                PlannedArgument::Literal { value } => {
                                    format!("  argv[{}] = {:?}", index + 1, value)
                                }
                                PlannedArgument::ArtifactPath { artifact } => format!(
                                    "  argv[{}] = owned artifact path ({artifact})",
                                    index + 1
                                ),
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        rows.push(CheckRow { id: format!("invocation:{}", invocation.id), title: format!("Invocation · {} · {}", invocation.id, invocation.command),
                        detail: format!("Command {}\nTool: {}\n{}\nWorking directory: {}\nTimeout: {}s · stdout {}B · stderr {}B\nEffects:\n{}\nInputs: {} entries · {}B · complete {}\nManifest: {}\nInvocation: {}\nArtifacts:\n{}", invocation.command, invocation.tool, args, invocation.cwd.display(), invocation.bounds.timeout_seconds, invocation.bounds.stdout_bytes, invocation.bounds.stderr_bytes, effects(&invocation.effects), invocation.inputs.entries.len(), invocation.inputs.total_bytes, invocation.inputs.complete, invocation.inputs.fingerprint, invocation.fingerprint,
                            invocation.artifacts.iter().map(|artifact| format!("{}: {} · {}B · required {}", artifact.id, artifact.name, artifact.max_bytes, artifact.required)).collect::<Vec<_>>().join("\n")), target: None });
                    }
                    for check in &plan.checks {
                        rows.push(CheckRow {
                            id: format!("planned-check:{}", check.id),
                            title: format!("Expected result · {}", check.id),
                            detail: expectation(&check.expectation),
                            target: None,
                        });
                    }
                }
            }
            ChecksPage::Results => {
                for run in &state.runs {
                    let id = &run.run.intent.id;
                    if state.filter.contains(run.assessment.state) {
                        rows.push(CheckRow {
                            id: format!("run:{id}"),
                            title: format!("{:?} · {id}", run.assessment.state),
                            detail: run_detail(run),
                            target: Some(CheckTarget::Run(id.clone())),
                        });
                    }
                    if let Some(result) = &run.results {
                        for check in &result.result.checks {
                            let current_state = match run.assessment.state {
                                RunState::Unknown | RunState::Stale => run.assessment.state,
                                _ => check.state,
                            };
                            if !state.filter.contains(current_state) {
                                continue;
                            }
                            let process = result.result.invocations.get(check.invocation);
                            let failures = check
                                .report
                                .failures
                                .iter()
                                .map(|failure| format!("{}: {}", failure.id, failure.message))
                                .collect::<Vec<_>>()
                                .join("\n");
                            rows.push(CheckRow { id: format!("result:{id}:{}", check.check.id), title: format!("{:?} · check {}", current_state, check.check.id),
                                detail: format!("Run {id}\nCheck {}\nCurrent run assessment: {:?}\nHistorical check: {:?} · report {:?}\nReasons: {}\n{}\nFailures ({} omitted):\n{}\n\nThese are local observations; they do not complete the issue or verify external evidence.", check.check.id, run.assessment.state, check.state, check.report.state, check.reason_codes.iter().chain(&check.report.reason_codes).cloned().collect::<Vec<_>>().join(" · "),
                                    process.map(|value| format!("Process {:?} · exit {:?} · cleanup {}\nInputs unchanged: {}\nstdout: {} ({}B retained, truncated {})\nstderr: {} ({}B retained, truncated {})", value.process.termination, value.process.exit_code, value.process.cleanup_complete, value.inputs_unchanged, value.process.stdout.path.display(), value.process.stdout.retained_bytes, value.process.stdout.truncated, value.process.stderr.path.display(), value.process.stderr.retained_bytes, value.process.stderr.truncated)).unwrap_or_else(|| "Process observation unavailable".into()),
                                    check.report.omitted_failures, excerpt(&failures)), target: Some(CheckTarget::Result(id.clone(), check.check.id.clone())) });
                        }
                    }
                }
            }
        }
        rows
    }
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(4),
        ])
        .split(area);
        let state = self.state();
        let page = state.map(|state| state.page).unwrap_or_default();
        let title = match page {
            ChecksPage::Definitions => "Check definitions",
            ChecksPage::Plan => "Inspected check plan",
            ChecksPage::Results => "Check results",
        };
        Paragraph::new(format!(
            "{title} · local feedback{}",
            if self.signal.is_active() {
                " · RUN ACTIVE"
            } else {
                ""
            }
        ))
        .style(style.add_modifier(Modifier::BOLD))
        .render(layout[0], buffer);
        let rows = self.rows();
        let state = self.state_mut();
        let index = state.page as usize;
        let selected = rows
            .iter()
            .position(|row| Some(&row.id) == state.selected[index].as_ref())
            .or_else(|| (!rows.is_empty()).then_some(0));
        let columns = if area.width >= 96 {
            Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(layout[1])
        } else {
            Layout::vertical([Constraint::Length(6), Constraint::Min(0)]).split(layout[1])
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
                    .title(format!(" {page:?} · {:?} ", state.filter)),
            )
            .highlight_symbol("› ")
            .highlight_style(style.add_modifier(Modifier::BOLD)),
            columns[0],
            buffer,
            &mut list_state,
        );
        state.offset[index] = list_state.offset();
        let detail = selected.and_then(|index| rows.get(index)).map(|row| row.detail.as_str()).unwrap_or(match page {
            ChecksPage::Definitions => "No definitions available. Add repository commands, checks and profiles, then press r. Discovery never executes recipes.",
            ChecksPage::Plan => "No inspected plan. d opens definitions; select a profile or check, then p builds its plan.",
            ChecksPage::Results => "No matching results among up to 100 recent runs. f changes the status filter; r refreshes retained results.",
        });
        Paragraph::new(safe(detail))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Inspected details "),
            )
            .style(style)
            .wrap(Wrap { trim: false })
            .scroll((state.scroll[index], 0))
            .render(columns[1], buffer);
        let mut footer = Vec::new();
        if let Some(error) = &state.error {
            footer.push(format!(
                "{} · {}",
                error.message,
                error
                    .hint
                    .as_deref()
                    .unwrap_or("Original plan and request retained.")
            ));
        }
        if let Some(notice) = &state.notice {
            footer.push(notice.clone());
        }
        footer.push("d definitions · p plan · b inspected plan · x run/retry · v results · f filter · e explain".into());
        footer.push("r refresh · D discard plan · PgUp/PgDn scroll · Ctrl-C cancel · 1–4 context · Esc back".into());
        Paragraph::new(safe(&footer.join("\n")))
            .style(style)
            .wrap(Wrap { trim: false })
            .render(layout[2], buffer);
    }
}

fn run_detail(run: &RunOutcome) -> String {
    format!(
        "Run {}\nCurrent assessment: {:?}\nHistorical result: {:?}\nBasis: {}\nReasons: {}\nPlan: {}\nRequest: {}\n{}\nArtifacts:\n{}\nReceipts: {}\nEnter/e refreshes current availability and source freshness. Raw logs are not loaded into context.",
        run.run.intent.id,
        run.assessment.state,
        run.assessment.historical_state,
        run.assessment.basis,
        run.assessment.reason_codes.join(" · "),
        run.run.intent.input.expected_plan,
        run.run.intent.request_id,
        if run.replayed {
            "Original request replayed; no new process started."
        } else {
            "Explicit foreground invocation."
        },
        run.assessment
            .artifacts
            .iter()
            .map(|artifact| format!(
                "{} · {:?} · {}",
                artifact.id,
                artifact.availability,
                artifact.path.display()
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        run.receipts
            .iter()
            .map(|receipt| receipt.operation_id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}
fn expectation(value: &ReportExpectation) -> String {
    match value {
        ReportExpectation::Process { allowed_exit_codes } => {
            format!("Process result · allowed exits {allowed_exit_codes:?}")
        }
        ReportExpectation::JUnit {
            artifact,
            suites,
            minimum_tests,
            maximum_skipped,
            allowed_exit_codes,
        } => format!(
            "JUnit artifact {artifact}\nRequired suites: {}\nMinimum tests: {minimum_tests}\nMaximum skipped: {maximum_skipped:?}\nAllowed exits: {allowed_exit_codes:?}\nExit zero alone cannot prove this suite ran.",
            suites.join(", ")
        ),
        ReportExpectation::Sarif {
            artifact,
            tool,
            minimum_invocations,
            failure_levels,
            allowed_exit_codes,
        } => format!(
            "SARIF artifact {artifact}\nTool: {tool}\nMinimum invocations: {minimum_invocations}\nFailure levels: {failure_levels:?}\nAllowed exits: {allowed_exit_codes:?}"
        ),
    }
}
fn effects(values: &[DeclaredEffect]) -> String {
    values
        .iter()
        .map(|value| match value {
            DeclaredEffect::Read { path } => format!("read {}", path.display()),
            DeclaredEffect::Write { path } => format!("write {}", path.display()),
            DeclaredEffect::Network { description } => format!("network {}", excerpt(description)),
            DeclaredEffect::Other { description } => excerpt(description),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn excerpt(value: &str) -> String {
    if value.chars().count() > 8192 {
        format!(
            "{}\n[detail excerpt truncated]",
            value.chars().take(8192).collect::<String>()
        )
    } else {
        value.into()
    }
}
fn safe(value: &str) -> String {
    value
        .lines()
        .map(sanitize_terminal_line)
        .collect::<Vec<_>>()
        .join("\n")
}
