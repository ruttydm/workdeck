//! Host-owned chrome for extension input and selection dialogs.
//!
//! This is the native Ratatui translation of Hunk's MIT-licensed
//! `src/ui/components/chrome/ExtensionDialog.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. The extension supplies only
//! text and choices; Workdeck owns dimensions, attribution, clipping, paint,
//! pointer hit maps, actions, and dismissal.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::extension_dialogs::{ExtensionInputDialog, ExtensionSelectDialog};
use crate::{
    AppTheme, ConfirmDialogAction, DialogActionHit, ModalFrameOptions, ModalFramePlan,
    extension_toast_prefix, fit_text, list_window_start, pad_text, paint_dialog_action_row,
    ratatui_theme_color, render_modal_frame,
};

const EXTENSION_DIALOG_MAX_WIDTH: u16 = 72;
const EXTENSION_DIALOG_MIN_WIDTH: u16 = 40;
const EXTENSION_DIALOG_HORIZONTAL_MARGIN: u16 = 8;
const EXTENSION_SELECT_MIN_HEIGHT: u16 = 11;
const EXTENSION_SELECT_MAX_HEIGHT: u16 = 24;
const EXTENSION_SELECT_VERTICAL_MARGIN: u16 = 6;
const ATTRIBUTED_INPUT_HEIGHT: u16 = 10;
const INPUT_HEIGHT: u16 = 8;
const SELECT_MARKER_WIDTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionSelectItemHit {
    pub index: usize,
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionSelectDialogPlan {
    pub modal: ModalFramePlan,
    pub body_width: usize,
    pub content_rows: usize,
    pub attribution_rows: usize,
    pub visible_rows: usize,
    pub window_start: usize,
    pub status_rows: usize,
    pub action_gap_rows: usize,
    pub action_rows: usize,
    pub item_hits: Vec<ExtensionSelectItemHit>,
    pub action_hits: Vec<DialogActionHit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionInputDialogPlan {
    pub modal: ModalFramePlan,
    pub body_width: usize,
    pub content_rows: usize,
    pub attribution_rows: usize,
    pub attribution_gap_rows: usize,
    pub field_rows: usize,
    pub field_gap_rows: usize,
    pub action_rows: usize,
    pub field: Option<Rect>,
    pub action_hits: Vec<DialogActionHit>,
}

#[must_use]
pub(crate) fn extension_dialog_width(terminal_width: u16) -> u16 {
    EXTENSION_DIALOG_MAX_WIDTH.min(
        EXTENSION_DIALOG_MIN_WIDTH.max(
            terminal_width.saturating_sub(EXTENSION_DIALOG_HORIZONTAL_MARGIN.min(terminal_width)),
        ),
    )
}

fn fixed_child_row(frame: Rect, child_index: usize, width: u16) -> Rect {
    let y = frame
        .y
        .saturating_add(4)
        .saturating_add(u16::try_from(child_index).unwrap_or(u16::MAX));
    Rect::new(
        frame.x.saturating_add(2),
        y,
        width,
        u16::from(width > 0 && y < frame.y.saturating_add(frame.height)),
    )
}

fn paint_row(buffer: &mut Buffer, row: Rect, text: &str, foreground: &str, background: &str) {
    if row.width == 0 || row.height == 0 {
        return;
    }
    let style = Style::default()
        .fg(ratatui_theme_color(foreground))
        .bg(ratatui_theme_color(background));
    buffer.set_style(row, style);
    buffer.set_stringn(row.x, row.y, text, usize::from(row.width), style);
}

fn attribution_text(extension_id: &str, width: usize) -> String {
    fit_text(
        &format!("{} {extension_id}", extension_toast_prefix()),
        width,
        None,
    )
}

/// Paint a selection dialog and return every row/action pointer target.
pub(crate) fn render_extension_select_dialog_view(
    area: Rect,
    buffer: &mut Buffer,
    dialog: &ExtensionSelectDialog,
    hovered_action_key: Option<&str>,
    theme: &AppTheme,
) -> ExtensionSelectDialogPlan {
    let requested_height = EXTENSION_SELECT_MIN_HEIGHT
        .max(area.height.saturating_sub(EXTENSION_SELECT_VERTICAL_MARGIN))
        .min(EXTENSION_SELECT_MAX_HEIGHT);
    let modal = render_modal_frame(
        area,
        buffer,
        ModalFrameOptions {
            width: extension_dialog_width(area.width),
            height: requested_height,
            closeable: true,
            has_mouse_scroll_handler: false,
            terminal_width: area.width,
            terminal_height: area.height,
            theme,
            title: &dialog.title,
        },
    );
    let body_width = usize::from(modal.frame.width.saturating_sub(4).max(1));
    let content_rows = usize::from(
        modal
            .frame
            .height
            .saturating_sub(crate::ui_geometry::MODAL_FRAME_CHROME_ROWS),
    );
    let attribution_rows = usize::from(dialog.show_attribution && content_rows >= 2);
    let action_rows = usize::from(content_rows.saturating_sub(attribution_rows) >= 2);
    let status_rows = usize::from(
        content_rows
            .saturating_sub(attribution_rows)
            .saturating_sub(action_rows)
            >= 2,
    );
    let action_gap_rows = usize::from(
        content_rows
            .saturating_sub(attribution_rows)
            .saturating_sub(action_rows)
            .saturating_sub(status_rows)
            >= 2,
    );
    let visible_rows = content_rows
        .saturating_sub(attribution_rows)
        .saturating_sub(action_rows)
        .saturating_sub(status_rows)
        .saturating_sub(action_gap_rows);
    let window_start =
        list_window_start(dialog.selected, dialog.options.len(), visible_rows.max(1));
    let mut child_index = 0;
    if attribution_rows > 0 {
        let row = fixed_child_row(
            modal.frame,
            child_index,
            modal.frame.width.saturating_sub(4),
        );
        paint_row(
            buffer,
            row,
            &attribution_text(&dialog.extension_id, body_width),
            &theme.badge_neutral,
            &theme.panel,
        );
        child_index += 1;
    }

    let label_width = body_width.saturating_sub(SELECT_MARKER_WIDTH).max(4);
    let mut item_hits = Vec::new();
    for (offset, option) in dialog
        .options
        .iter()
        .skip(window_start)
        .take(visible_rows)
        .enumerate()
    {
        let index = window_start + offset;
        let selected = index == dialog.selected;
        let row = fixed_child_row(
            modal.frame,
            child_index,
            modal.frame.width.saturating_sub(4),
        );
        let foreground = if selected { &theme.text } else { &theme.muted };
        let background = if selected {
            &theme.accent_muted
        } else {
            &theme.panel
        };
        let marker = pad_text(if selected { "›" } else { " " }, SELECT_MARKER_WIDTH);
        let label = fit_text(option, label_width, None);
        paint_row(
            buffer,
            row,
            &format!("{marker}{label}"),
            foreground,
            background,
        );
        if row.height > 0 {
            item_hits.push(ExtensionSelectItemHit { index, bounds: row });
        }
        child_index += 1;
    }

    if status_rows > 0 {
        let row = fixed_child_row(
            modal.frame,
            child_index,
            modal.frame.width.saturating_sub(4),
        );
        let status = format!(
            "{}-{} of {}",
            window_start.saturating_add(1),
            window_start
                .saturating_add(visible_rows)
                .min(dialog.options.len()),
            dialog.options.len()
        );
        paint_row(
            buffer,
            row,
            &fit_text(&status, body_width, None),
            &theme.muted,
            &theme.panel,
        );
        child_index += 1;
    }
    child_index = child_index.saturating_add(action_gap_rows);
    let action_hits = if action_rows > 0 {
        let row = fixed_child_row(
            modal.frame,
            child_index,
            modal.frame.width.saturating_sub(4),
        );
        paint_dialog_action_row(
            row,
            buffer,
            &[
                ConfirmDialogAction::new("enter", "choose"),
                ConfirmDialogAction::new("esc", "cancel"),
            ],
            hovered_action_key,
            theme,
        )
    } else {
        Vec::new()
    };

    ExtensionSelectDialogPlan {
        modal,
        body_width,
        content_rows,
        attribution_rows,
        visible_rows,
        window_start,
        status_rows,
        action_gap_rows,
        action_rows,
        item_hits,
        action_hits,
    }
}

/// Paint a focused one-line input dialog and return field/action geometry.
pub(crate) fn render_extension_input_dialog_view(
    area: Rect,
    buffer: &mut Buffer,
    dialog: &ExtensionInputDialog,
    hovered_action_key: Option<&str>,
    theme: &AppTheme,
) -> ExtensionInputDialogPlan {
    let modal = render_modal_frame(
        area,
        buffer,
        ModalFrameOptions {
            width: extension_dialog_width(area.width),
            height: if dialog.show_attribution {
                ATTRIBUTED_INPUT_HEIGHT
            } else {
                INPUT_HEIGHT
            },
            closeable: true,
            has_mouse_scroll_handler: false,
            terminal_width: area.width,
            terminal_height: area.height,
            theme,
            title: &dialog.title,
        },
    );
    let body_width = usize::from(modal.frame.width.saturating_sub(4).max(1));
    let content_rows = usize::from(
        modal
            .frame
            .height
            .saturating_sub(crate::ui_geometry::MODAL_FRAME_CHROME_ROWS),
    );
    let attribution_rows = usize::from(dialog.show_attribution && content_rows >= 2);
    let action_rows = usize::from(content_rows.saturating_sub(attribution_rows) >= 2);
    let attribution_gap_rows = usize::from(
        content_rows
            .saturating_sub(attribution_rows)
            .saturating_sub(action_rows)
            >= 2,
    );
    let field_gap_rows = usize::from(
        content_rows
            .saturating_sub(attribution_rows)
            .saturating_sub(action_rows)
            .saturating_sub(attribution_gap_rows)
            >= 2,
    );
    let field_rows = content_rows
        .saturating_sub(attribution_rows)
        .saturating_sub(action_rows)
        .saturating_sub(attribution_gap_rows)
        .saturating_sub(field_gap_rows);
    let mut child_index = 0;
    if attribution_rows > 0 {
        let row = fixed_child_row(
            modal.frame,
            child_index,
            modal.frame.width.saturating_sub(4),
        );
        paint_row(
            buffer,
            row,
            &attribution_text(&dialog.extension_id, body_width),
            &theme.badge_neutral,
            &theme.panel,
        );
        child_index += 1;
    }
    child_index = child_index.saturating_add(attribution_gap_rows);
    let field = (field_rows > 0).then(|| {
        let row = fixed_child_row(
            modal.frame,
            child_index,
            modal.frame.width.saturating_sub(4),
        );
        let (text, foreground) = if dialog.value.is_empty() {
            (dialog.placeholder.as_str(), theme.muted.as_str())
        } else {
            (dialog.value.as_str(), theme.text.as_str())
        };
        paint_row(
            buffer,
            row,
            &fit_text(text, body_width, None),
            foreground,
            &theme.panel_alt,
        );
        row
    });
    child_index = child_index
        .saturating_add(usize::from(field_rows > 0))
        .saturating_add(field_gap_rows);
    let action_hits = if action_rows > 0 {
        let row = fixed_child_row(
            modal.frame,
            child_index,
            modal.frame.width.saturating_sub(4),
        );
        paint_dialog_action_row(
            row,
            buffer,
            &[
                ConfirmDialogAction::new("enter", "submit"),
                ConfirmDialogAction::new("esc", "cancel"),
            ],
            hovered_action_key,
            theme,
        )
    } else {
        Vec::new()
    };

    ExtensionInputDialogPlan {
        modal,
        body_width,
        content_rows,
        attribution_rows,
        attribution_gap_rows,
        field_rows,
        field_gap_rows,
        action_rows,
        field,
        action_hits,
    }
}

#[must_use]
pub(crate) fn select_item_at(
    plan: &ExtensionSelectDialogPlan,
    column: u16,
    row: u16,
) -> Option<&ExtensionSelectItemHit> {
    plan.item_hits.iter().find(|hit| {
        column >= hit.bounds.x
            && column < hit.bounds.right()
            && row >= hit.bounds.y
            && row < hit.bounds.bottom()
    })
}

#[must_use]
pub(crate) fn extension_dialog_action_at(
    hits: &[DialogActionHit],
    column: u16,
    row: u16,
) -> Option<&DialogActionHit> {
    hits.iter().find(|hit| {
        column >= hit.bounds.x
            && column < hit.bounds.right()
            && row >= hit.bounds.y
            && row < hit.bounds.bottom()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_theme;

    #[test]
    fn frozen_oracle_records_both_pins_frames_and_application_interactions() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../port/hunk/oracles/extension-dialog-view.json"
        )))
        .unwrap();
        assert_eq!(
            oracle["source"]["baselineAndStableBlob"],
            "5a79b6b79c3d56465e001e6977aed53bc6b608cc"
        );
        assert_eq!(oracle["directOracle"]["eachPin"]["passed"], 1);
        assert_eq!(
            oracle["directOracle"]["frames"].as_array().unwrap().len(),
            4
        );
        assert_eq!(oracle["applicationOracle"]["eachPin"]["passed"], 8);
        assert_eq!(
            oracle["applicationOracle"]["tests"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
    }

    fn frame(buffer: &Buffer, width: u16) -> String {
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn options() -> Vec<String> {
        (0..12).map(|index| format!("Option {index}")).collect()
    }

    fn select_dialog() -> ExtensionSelectDialog {
        ExtensionSelectDialog {
            request_id: 1,
            extension_index: 0,
            extension_id: "team-tools".into(),
            action_id: "ask".into(),
            show_attribution: true,
            title: "Deploy where?".into(),
            options: options(),
            selected: 8,
        }
    }

    fn input_dialog(value: &str) -> ExtensionInputDialog {
        ExtensionInputDialog {
            request_id: 2,
            extension_index: 0,
            extension_id: "branch-tools".into(),
            action_id: "ask".into(),
            show_attribution: true,
            title: "Branch name?".into(),
            placeholder: "feature/...".into(),
            value: value.into(),
        }
    }

    #[test]
    fn roomy_select_matches_the_frozen_opentui_cell_frame() {
        let area = Rect::new(0, 0, 80, 24);
        let mut buffer = Buffer::empty(area);
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let plan =
            render_extension_select_dialog_view(area, &mut buffer, &select_dialog(), None, &theme);
        let output = frame(&buffer, area.width);
        assert_eq!(plan.modal.frame, Rect::new(4, 3, 72, 18));
        assert_eq!(plan.visible_rows, 9);
        assert_eq!(plan.window_start, 3);
        assert_eq!(plan.item_hits.first().unwrap().index, 3);
        assert_eq!(plan.item_hits.last().unwrap().index, 11);
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n").as_bytes()),
            "8052b0d091c40d508fd5f98f309354471bd5a48d3f5148e284c3b61e1b3d5b38",
            "{output}"
        );
    }

    #[test]
    fn narrow_select_preserves_only_attribution_selected_row_and_actions() {
        let area = Rect::new(0, 0, 50, 10);
        let mut buffer = Buffer::empty(area);
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let plan =
            render_extension_select_dialog_view(area, &mut buffer, &select_dialog(), None, &theme);
        let output = frame(&buffer, area.width);
        assert_eq!(plan.modal.frame, Rect::new(4, 1, 42, 8));
        assert_eq!(plan.visible_rows, 1);
        assert_eq!(plan.window_start, 8);
        assert_eq!(plan.item_hits[0].index, 8);
        assert_eq!(plan.action_hits.len(), 2);
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n").as_bytes()),
            "1fef3064ab1ac7dc93583e869dd1c2e604f531d6df5056d9c5a23ee83c57bf85",
            "{output}"
        );
    }

    #[test]
    fn attributed_input_matches_the_frozen_opentui_cell_frame() {
        let area = Rect::new(0, 0, 80, 20);
        let mut buffer = Buffer::empty(area);
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let plan = render_extension_input_dialog_view(
            area,
            &mut buffer,
            &input_dialog("quick-fix"),
            None,
            &theme,
        );
        let output = frame(&buffer, area.width);
        assert_eq!(plan.modal.frame, Rect::new(4, 5, 72, 10));
        assert_eq!(plan.field, Some(Rect::new(6, 11, 68, 1)));
        assert_eq!(plan.action_hits.len(), 2);
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n").as_bytes()),
            "ffd02d648a9a4ba264d59e6c98d9c8dd60de4605cb2d5126ea1a121c883321e4",
            "{output}"
        );
    }

    #[test]
    fn cramped_input_flexes_every_child_away_without_overwriting_the_frame() {
        let area = Rect::new(0, 0, 38, 7);
        let mut buffer = Buffer::empty(area);
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let plan =
            render_extension_input_dialog_view(area, &mut buffer, &input_dialog(""), None, &theme);
        let output = frame(&buffer, area.width);
        assert_eq!(plan.modal.frame, Rect::new(1, 1, 36, 5));
        assert_eq!(plan.content_rows, 0);
        assert!(plan.field.is_none());
        assert!(plan.action_hits.is_empty());
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n").as_bytes()),
            "a62c325b6afe5458dd524eee2c7bf7de0ea70771f5d39fb39b53b91f422b95ba",
            "{output}"
        );
    }

    #[test]
    fn pointer_hit_maps_cover_only_complete_select_and_action_rows() {
        let area = Rect::new(0, 0, 50, 10);
        let mut buffer = Buffer::empty(area);
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let plan =
            render_extension_select_dialog_view(area, &mut buffer, &select_dialog(), None, &theme);
        let hit = &plan.item_hits[0];
        assert_eq!(
            select_item_at(&plan, hit.bounds.x, hit.bounds.y).map(|hit| hit.index),
            Some(8)
        );
        assert_eq!(
            select_item_at(&plan, hit.bounds.right() - 1, hit.bounds.y).map(|hit| hit.index),
            Some(8)
        );
        assert!(select_item_at(&plan, hit.bounds.x, plan.modal.frame.y).is_none());
    }
}
