//! Retained, read-only activity navigation shares the bounded projection worker.
use super::{WorkbenchShell, WorkbenchTab, indexed_workspace::IndexedWorkspace};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    widgets::{Paragraph, Widget},
};
use workdeck_pm::{
    SourceSelector,
    projection::{ProjectionActivityQuery, ProjectionLimits, ProjectionQuery},
};

impl WorkbenchShell {
    pub(super) fn open_activity(&mut self) {
        if self.activity.is_none() {
            match IndexedWorkspace::new(
                self.options.root.clone(),
                SourceSelector::WorkingTree,
                ProjectionQuery::Activity {
                    query: ProjectionActivityQuery::default(),
                },
                ProjectionLimits::default(),
            ) {
                Ok(mut activity) => {
                    activity.refresh(false);
                    self.activity = Some(activity);
                }
                Err(error) => {
                    self.notice = Some(error.to_string());
                    return;
                }
            }
        }
        self.effect = Some(super::shell::ShellEffect::Return);
        self.tab = WorkbenchTab::Activity;
    }
    pub(super) fn render_activity(
        &mut self,
        area: Rect,
        buffer: &mut Buffer,
        theme: &crate::AppTheme,
    ) {
        self.planning_bounds = Some(area);
        self.rendered = None;
        let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
        self.index_rendered = self
            .activity
            .as_mut()
            .map(|activity| super::indexed_view::render(activity, rows[0], buffer, theme));
        Paragraph::new(
            "Activity · newest first · r refresh · Enter source · Esc/F3 Issues · F2 Review",
        )
        .render(rows[1], buffer);
    }
    pub(super) fn activity_mouse(&mut self, event: &MouseEvent) -> bool {
        let contains = |area: Rect| area.contains((event.column, event.row).into());
        let Some(rendered) = &self.index_rendered else {
            return false;
        };
        let clicked = rendered
            .rows
            .iter()
            .find(|(area, _)| contains(*area))
            .map(|(_, token)| token.clone());
        let over_list = contains(rendered.list);
        let over_detail = contains(rendered.detail);
        let Some(activity) = &mut self.activity else {
            return false;
        };
        match event.kind {
            MouseEventKind::ScrollDown if over_detail => {
                activity.detail_scroll = activity
                    .detail_scroll
                    .saturating_add(3)
                    .min(activity.opened_lines.len().saturating_sub(1))
            }
            MouseEventKind::ScrollUp if over_detail => {
                activity.detail_scroll = activity.detail_scroll.saturating_sub(3)
            }
            MouseEventKind::ScrollDown if over_list => activity.move_by(1),
            MouseEventKind::ScrollUp if over_list => activity.move_by(-1),
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(token) = clicked
                    && activity.select_token(&token)
                {
                    activity.open_selected();
                }
            }
            _ => {}
        }
        over_list || over_detail
    }
}
