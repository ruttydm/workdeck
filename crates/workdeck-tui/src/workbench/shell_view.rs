use super::{WorkbenchShell, WorkbenchTab, render_workbench_buffer};
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;

impl WorkbenchShell {
    pub(super) fn render_nav(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        self.nav_bounds = area;
        let normal = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        let selected = normal
            .add_modifier(Modifier::BOLD)
            .fg(ratatui_theme_color(&theme.accent));
        self.nav_hits.clear();
        let first = [
            (
                if self.context.checks.signal.is_active() {
                    " F2 Review · CHECK RUN ACTIVE ".to_owned()
                } else {
                    " F2 Review ".to_owned()
                },
                crossterm::event::KeyCode::F(2),
                self.tab == WorkbenchTab::Review,
            ),
            (
                match &self.index {
                    Some(index) if index.stale() => " F3 Issues · stale ".to_owned(),
                    Some(index) if index.handle.is_none() => " F3 Issues · loading ".to_owned(),
                    _ => " F3 Issues ".to_owned(),
                },
                crossterm::event::KeyCode::F(3),
                self.tab == WorkbenchTab::Issues,
            ),
            (
                " F11 Projects ".to_owned(),
                crossterm::event::KeyCode::F(11),
                self.tab == WorkbenchTab::Planning
                    && self.planning.kind == workdeck_pm::PlanningKind::Project,
            ),
            (
                " F12 Cycles ".to_owned(),
                crossterm::event::KeyCode::F(12),
                self.tab == WorkbenchTab::Planning
                    && self.planning.kind == workdeck_pm::PlanningKind::Cycle,
            ),
            (
                " | F4 Issue from file/note · Shift-F4 link issue".to_owned(),
                crossterm::event::KeyCode::F(4),
                false,
            ),
        ];
        let second = [
            super::PanelPage::Changes,
            super::PanelPage::Git,
            super::PanelPage::Files,
            super::PanelPage::Agents,
            super::PanelPage::Search,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, page)| {
            (
                format!(" F{} {} ", index + 5, page.title()),
                crossterm::event::KeyCode::F(index as u8 + 5),
                self.tab == WorkbenchTab::Repository(page),
            )
        })
        .collect::<Vec<_>>();
        let mut lines = vec![first.to_vec()];
        if self.panels.is_some() {
            lines.push(second);
        }
        let menu_height = area
            .height
            .saturating_sub(u16::from(self.checkout_label.is_some()));
        for (row, entries) in lines.into_iter().take(usize::from(menu_height)).enumerate() {
            let mut x = area.x;
            for (label, key, active) in entries {
                let width = (label.len() as u16).min(area.right().saturating_sub(x));
                let rect = Rect::new(x, area.y + row as u16, width, 1);
                Paragraph::new(label)
                    .style(if active { selected } else { normal })
                    .render(rect, buffer);
                self.nav_hits.push((rect, key));
                x = x.saturating_add(width);
            }
        }
        if let Some(label) = &self.checkout_label
            && area.height > 0
        {
            Paragraph::new(sanitize_terminal_line(&format!(
                "Checkout: {label} · {}",
                self.options.root.display()
            )))
            .style(normal.add_modifier(Modifier::BOLD))
            .render(Rect::new(area.x, area.bottom() - 1, area.width, 1), buffer);
        }
    }

    pub(super) fn render_planning(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        self.planning_bounds = Some(area);
        self.index_rendered = None;
        if self.context_visible {
            self.rendered = None;
            self.context
                .render(area, buffer, theme, self.notice.as_deref());
            return;
        }
        if let Some(graph) = &mut self.graph {
            self.rendered = None;
            graph.render(area, buffer, theme);
            return;
        }
        let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        if !self.available {
            let error = self.controller.error().map(|error| &error.error);
            let message = error
                .map(|error| sanitize_terminal_line(&error.message))
                .unwrap_or_else(|| "Planning source is unavailable".into());
            let hint = error
                .and_then(|error| error.hint.as_deref())
                .map(sanitize_terminal_line)
                .unwrap_or_else(|| {
                    "Run workdeck init --prefix WD, then press r to refresh.".into()
                });
            Paragraph::new(format!("{message}\n\n{hint}\n\nPlanning remains available after initialization or migration.\nPress F2 to review this repository."))
                .block(Block::default().title("Issues · source unavailable").borders(Borders::ALL)).style(style).wrap(Wrap{trim:false}).render(rows[0],buffer);
            self.rendered = None;
        } else if let Some(form) = &self.form {
            render_form(form, rows[0], buffer, theme);
            self.rendered = None;
        } else if let Some(index) = &mut self.index {
            self.rendered = None;
            self.index_rendered = Some(super::indexed_view::render(index, rows[0], buffer, theme));
        } else {
            self.rendered = Some(render_workbench_buffer(
                buffer,
                rows[0],
                &mut self.controller,
                theme,
            ));
        }
        let controls = if self
            .form
            .as_ref()
            .is_some_and(|form| matches!(form.kind, super::input::FormKind::LinkFile { .. }))
        {
            "Enter link · Ctrl-S save · Esc cancel · F2 Review"
        } else if self.form.is_some() {
            "Tab field · Ctrl-S save · Ctrl-U clear · Esc retain draft"
        } else {
            "n create · e edit · c comment · s status · a assign · p priority · l labels\n/ filter · x clear · r refresh · f file · g commit · i context · b graph · v features · Shift-F12 activity · Shift-F9 My work · d done · o reopen"
        };
        let mut footer = controls.to_owned();
        if let Some(notice) = &self.notice {
            footer = format!("{}\n{controls}", sanitize_terminal_line(notice));
        }
        Paragraph::new(footer)
            .style(style)
            .wrap(Wrap { trim: false })
            .render(rows[1], buffer);
    }
}

pub(super) fn render_form(
    form: &super::input::WorkbenchForm,
    area: Rect,
    buffer: &mut Buffer,
    theme: &AppTheme,
) {
    let style = Style::default()
        .fg(ratatui_theme_color(&theme.text))
        .bg(ratatui_theme_color(&theme.panel));
    // Keep a complete selected field visible when the form is taller
    // than the viewport. A field needs its border and one content row.
    let reserved = 1 + u16::from(!form.help.is_empty());
    let capacity = usize::from(area.height.saturating_sub(reserved) / 3).max(1);
    let count = form.fields.len().min(capacity);
    let start = form
        .selected
        .saturating_sub(count.saturating_sub(1))
        .min(form.fields.len().saturating_sub(count));
    let visible_fields = &form.fields[start..start + count];
    let show_help = !form.help.is_empty() && count == form.fields.len();
    let constraints = std::iter::once(Constraint::Length(1))
        .chain(visible_fields.iter().map(|field| {
            if field.multiline {
                Constraint::Min(3)
            } else {
                Constraint::Length(3)
            }
        }))
        .chain(show_help.then_some(Constraint::Min(1)))
        .collect::<Vec<_>>();
    let fields = Layout::vertical(constraints).split(area);
    Paragraph::new(if count < form.fields.len() {
        format!(
            "{} · field {}/{}",
            form.title,
            form.selected + 1,
            form.fields.len()
        )
    } else {
        form.title.clone()
    })
    .style(style.add_modifier(Modifier::BOLD))
    .render(fields[0], buffer);
    for (index, field) in visible_fields.iter().enumerate() {
        let selected = start + index == form.selected;
        let mut value = field.value.clone();
        if selected {
            let at = value
                .char_indices()
                .nth(field.cursor)
                .map_or(value.len(), |(at, _)| at);
            value.insert(at, '▏');
        }
        let value = value
            .lines()
            .map(sanitize_terminal_line)
            .collect::<Vec<_>>()
            .join("\n");
        let field_area = fields[index + 1];
        let cursor_row = field
            .value
            .chars()
            .take(field.cursor)
            .filter(|character| *character == '\n')
            .count();
        let scroll = cursor_row.saturating_sub(usize::from(field_area.height.saturating_sub(3)));
        Paragraph::new(value)
            .style(style)
            .block(
                Block::default()
                    .title(field.label)
                    .borders(Borders::ALL)
                    .border_style(if selected {
                        style.fg(ratatui_theme_color(&theme.accent))
                    } else {
                        style
                    }),
            )
            .wrap(Wrap { trim: false })
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .render(field_area, buffer);
    }
    if show_help {
        Paragraph::new(
            form.help
                .iter()
                .map(|line| sanitize_terminal_line(line))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .style(style)
        .wrap(Wrap { trim: false })
        .render(fields[count + 1], buffer);
    }
}
