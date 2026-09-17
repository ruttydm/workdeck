//! Bounded evidence presentation, independent of the retained exact source excerpt.
use super::MyWorkWorkspace;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};
use workdeck_diff::{TextSegment, wrap_segments};
const MAX_LINES: usize = 2048;
const MAX_BYTES: usize = 128 * 1024;
impl MyWorkWorkspace {
    pub(super) fn clear_evidence(&mut self) {
        self.evidence_lines.clear();
        self.evidence_scroll = 0;
    }
    pub(super) fn scroll_evidence(&mut self, delta: isize) {
        self.evidence_scroll = self
            .evidence_scroll
            .saturating_add_signed(delta)
            .min(self.evidence_lines.len().saturating_sub(1));
    }
    pub(crate) fn render_evidence(&mut self, area: Rect, buffer: &mut Buffer, style: Style) {
        let block = Block::default()
            .title("Work evidence · Shift-PgUp/PgDn")
            .borders(Borders::ALL);
        let inner = block.inner(area);
        block.style(style).render(area, buffer);
        if inner.width != self.evidence_width {
            self.clear_evidence();
            self.evidence_width = inner.width;
        }
        if self.evidence_lines.is_empty() {
            let row = self
                .report
                .as_ref()
                .and_then(|report| report.rows.get(self.selected));
            let mut used = 0usize;
            let mut omitted = false;
            let lines = &mut self.evidence_lines;
            let mut add = |text: String| {
                if used.saturating_add(text.len()) > MAX_BYTES || lines.len() >= MAX_LINES {
                    omitted = true;
                    return;
                }
                used += text.len();
                for line in wrap_segments(
                    vec![TextSegment { text, style: () }],
                    usize::from(inner.width).max(1),
                ) {
                    if lines.len() >= MAX_LINES {
                        omitted = true;
                        break;
                    }
                    lines.push(line.into_iter().map(|part| part.text).collect());
                }
            };
            if let Some(row) = row {
                add(format!("{} · {}", row.alias, row.row.title));
                if let Some(claim) = &row.evidence.claim {
                    add(format!(
                        "Claim actor: {} · {:?}",
                        claim.claim.metadata.actor, claim.claim.metadata.state
                    ));
                    add(format!("Assessment: {:?}", claim.assessment.disposition));
                    let scope = match claim.assessment.guarantee {
                        workdeck_pm::ClaimGuarantee::LocalSourceOnly => "local source only",
                        workdeck_pm::ClaimGuarantee::Unconfirmed => {
                            "unconfirmed shared observation"
                        }
                        workdeck_pm::ClaimGuarantee::SharedConfirmed => "shared confirmed",
                    };
                    add(format!("Observation scope: {scope}"));
                    let matching = match row.evidence.selected_requirements_match {
                        Some(true) => "yes",
                        Some(false) => "no; review changed requirements",
                        None => "unavailable",
                    };
                    add(format!("Selected requirements match: {matching}"));
                    add(format!(
                        "Expires: {}",
                        claim.claim.metadata.expires_at.to_rfc3339()
                    ));
                    add(format!(
                        "Assessed: {}",
                        claim.assessment.assessed_at.to_rfc3339()
                    ));

                    add(format!("Claim source: {:?}", claim.source.identity.role));
                    for reason in &claim.assessment.reason_codes {
                        add(reason.clone());
                    }
                }
                if let Some(readiness) = &row.evidence.readiness {
                    for condition in &readiness.conditions {
                        add(format!("{} · {:?}", condition.reason_code, condition.state));
                        add(condition.message.clone());
                    }
                }
                for question in &row.evidence.questions {
                    add(format!(
                        "Question {} · {:?} · {:?}",
                        question.question_id, question.state, question.freshness
                    ));
                    if question.blocks_implementation {
                        add("This question blocks implementation.".into());
                    }
                    for reason in &question.stale_reasons {
                        add(format!("{}: {}", reason.code, reason.message));
                    }
                }
                if row.evidence.is_empty() {
                    add("No supplemental evidence in this view.".into());
                }
            } else {
                add("No work selected.".into());
            }
            if omitted {
                self.evidence_lines
                    .push("More evidence is available in the JSON report.".into());
            }
        }
        let lines = self
            .evidence_lines
            .iter()
            .skip(self.evidence_scroll)
            .take(usize::from(inner.height))
            .map(|line| Line::from(line.as_str()))
            .collect::<Vec<_>>();
        Paragraph::new(lines).style(style).render(inner, buffer);
    }
}
