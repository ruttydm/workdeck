use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use workdeck_core::{Changeset, ChangesetSource, SidebarVisibility};
use workdeck_diff::parse_patch;
use workdeck_review::LayoutMode;
use workdeck_tui::{CursorLineMode, ReviewApp, ReviewOptions, measure_text_width, render};

const HEIGHT: u16 = 26;

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-selection.json"
    ))
    .unwrap()
}

fn dense_selection_changeset() -> Changeset {
    let mut patch = String::from(
        "diff --git a/selection.ts b/selection.ts\n--- a/selection.ts\n+++ b/selection.ts\n@@ -1,24 +1,24 @@\n",
    );
    for index in 1..=24 {
        patch.push_str(&format!(
            "-export const item{index:02} = {index};\n+export const item{index:02} = {};\n",
            index * 1_000
        ));
    }
    parse_patch(
        &patch,
        "changeset:copy-selection",
        "Copy selection",
        ChangesetSource::Patch {
            label: "copy-selection".into(),
        },
    )
    .unwrap()
}

fn wide_selection_changeset() -> Changeset {
    parse_patch(
        "diff --git a/i18n.ts b/i18n.ts\n--- a/i18n.ts\n+++ b/i18n.ts\n@@ -1 +1 @@\n-export const message = 'hello'; // greeting\n+export const message = 'こんにちは'; // greeting\n",
        "changeset:copy-selection-cjk",
        "CJK copy selection",
        ChangesetSource::Patch {
            label: "copy-selection-cjk".into(),
        },
    )
    .unwrap()
}

struct Fixture {
    app: ReviewApp,
    terminal: Terminal<TestBackend>,
}

impl Fixture {
    fn new(
        changeset: Changeset,
        layout: LayoutMode,
        width: u16,
        cursor_line: CursorLineMode,
        clipboard_supported: bool,
    ) -> Self {
        let mut app = ReviewApp::new(
            changeset,
            ReviewOptions {
                layout,
                sidebar_visibility: SidebarVisibility::Hidden,
                sidebar: false,
                line_number_digits: Some(2),
                cursor_line,
                highlight: false,
                copy_decorations: true,
                prompt_save_view_preferences: false,
                ..ReviewOptions::default()
            },
        );
        app.set_clipboard_copy_supported(clipboard_supported);
        Self {
            app,
            terminal: Terminal::new(TestBackend::new(width, HEIGHT)).unwrap(),
        }
    }

    fn draw(&mut self) -> String {
        self.terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &self.app))
            .unwrap();
        let buffer = self.terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn locate(&mut self, needle: &str) -> (u16, u16) {
        let frame = self.draw();
        frame
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                line.find(needle).map(|byte_index| {
                    (
                        u16::try_from(measure_text_width(&line[..byte_index])).unwrap(),
                        u16::try_from(row).unwrap(),
                    )
                })
            })
            .unwrap_or_else(|| panic!("{needle:?} was not visible in frame:\n{frame}"))
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        self.app.handle_mouse_event(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn click(&mut self, point: (u16, u16)) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), point.0, point.1);
        self.mouse(MouseEventKind::Up(MouseButton::Left), point.0, point.1);
    }

    fn drag(&mut self, start: (u16, u16), end: (u16, u16)) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), start.0, start.1);
        self.mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1);
        self.mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1);
    }
}

#[test]
fn frozen_app_host_selection_oracle_maps_identical_pins_and_all_source_tests() {
    let oracle = oracle();
    assert_eq!(
        oracle["source"]["baseline"]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["source"]["stable"]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_eq!(
        oracle["source"]["baseline"]["blob"],
        oracle["source"]["stable"]["blob"]
    );
    assert_eq!(oracle["source"]["baseline"]["bytes"], 16_221);
    assert_eq!(oracle["source"]["baseline"]["lines"], 483);
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 12);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 12);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 12);
}

#[test]
fn live_ratatui_drag_copies_changed_rows_and_uses_mouse_up_for_coalesced_motion() {
    let mut fixture = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Row,
        true,
    );
    let first = fixture.locate("item01");
    let third = fixture.locate("item03");
    let fifth = fixture.locate("item05");
    let painted_probe = (third.0.saturating_add(2), third.1);
    let before_background = fixture.terminal.backend().buffer()[painted_probe].bg;
    fixture.mouse(
        MouseEventKind::Down(MouseButton::Left),
        first.0.saturating_add(2),
        first.1,
    );
    fixture.mouse(
        MouseEventKind::Drag(MouseButton::Left),
        fifth.0.saturating_add(4),
        fifth.1,
    );
    fixture.draw();
    assert_ne!(
        fixture.terminal.backend().buffer()[painted_probe].bg,
        before_background
    );
    fixture.mouse(
        MouseEventKind::Up(MouseButton::Left),
        fifth.0.saturating_add(4),
        fifth.1,
    );
    let copied = fixture.app.take_clipboard_copy_request().unwrap();
    assert!(copied.contains("item"));
    assert!(fixture.draw().contains("Copied selection to clipboard"));

    let mut coalesced = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Row,
        true,
    );
    let first = coalesced.locate("item01");
    let second = coalesced.locate("item02");
    let fifth = coalesced.locate("item05");
    coalesced.mouse(
        MouseEventKind::Down(MouseButton::Left),
        first.0.saturating_add(2),
        first.1,
    );
    coalesced.mouse(
        MouseEventKind::Drag(MouseButton::Left),
        second.0.saturating_add(2),
        second.1,
    );
    coalesced.mouse(
        MouseEventKind::Up(MouseButton::Left),
        fifth.0.saturating_add(8),
        fifth.1,
    );
    assert!(
        coalesced
            .app
            .take_clipboard_copy_request()
            .unwrap()
            .contains("item05")
    );
}

#[test]
fn live_ratatui_copy_is_terminal_cell_exact_across_wide_cjk_clusters() {
    let mut fixture = Fixture::new(
        wide_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Row,
        true,
    );
    let wide = fixture.locate("こ");
    let prefix = "export const message = '";
    let start = (
        wide.0
            .saturating_sub(u16::try_from(measure_text_width(prefix)).unwrap()),
        wide.1,
    );
    let through_wide = "export const message = 'こんにちは";
    fixture.drag(
        start,
        (
            start
                .0
                .saturating_add(u16::try_from(measure_text_width(through_wide) - 1).unwrap()),
            start.1,
        ),
    );
    assert_eq!(
        fixture.app.take_clipboard_copy_request().as_deref(),
        Some(through_wide)
    );
}

#[test]
fn repeated_clicks_copy_one_word_then_the_complete_rendered_line() {
    let mut fixture = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Row,
        true,
    );
    let item = fixture.locate("item05");
    let target = (item.0.saturating_add(2), item.1);
    fixture.click(target);
    assert!(fixture.app.take_clipboard_copy_request().is_none());
    fixture.click(target);
    let word = fixture.app.take_clipboard_copy_request().unwrap();
    assert!(word.contains("item"));
    assert!(!word.contains(' '));
    fixture.click(target);
    let line = fixture.app.take_clipboard_copy_request().unwrap();
    assert!(line.contains("item05"));
    assert!(line.contains('='));
}

#[test]
fn click_slop_buttons_viewport_and_clipboard_capability_match_hunk() {
    let mut fixture = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Row,
        true,
    );
    let item = fixture.locate("item07");
    let target = (item.0.saturating_add(2), item.1);
    fixture.click(target);
    assert!(fixture.app.take_clipboard_copy_request().is_none());

    fixture.mouse(MouseEventKind::Down(MouseButton::Left), 40, 0);
    fixture.mouse(MouseEventKind::Up(MouseButton::Left), 40, 0);
    assert!(fixture.app.take_clipboard_copy_request().is_none());

    fixture.mouse(MouseEventKind::Down(MouseButton::Right), target.0, target.1);
    fixture.mouse(
        MouseEventKind::Drag(MouseButton::Right),
        target.0.saturating_add(8),
        target.1.saturating_add(2),
    );
    fixture.mouse(
        MouseEventKind::Up(MouseButton::Right),
        target.0.saturating_add(8),
        target.1.saturating_add(2),
    );
    assert!(fixture.app.take_clipboard_copy_request().is_none());

    let mut cursor_off = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Off,
        true,
    );
    let item = cursor_off.locate("item07");
    cursor_off.drag(
        (item.0.saturating_add(2), item.1),
        (item.0.saturating_add(3), item.1),
    );
    assert!(cursor_off.app.take_clipboard_copy_request().is_some());

    let mut unsupported = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Row,
        false,
    );
    let second = unsupported.locate("item02");
    let sixth = unsupported.locate("item06");
    unsupported.drag(
        (second.0.saturating_add(2), second.1),
        (sixth.0.saturating_add(4), sixth.1),
    );
    assert!(unsupported.app.take_clipboard_copy_request().is_none());
    assert!(unsupported.draw().contains("Clipboard copy unsupported"));
}

#[test]
fn file_header_and_split_side_drags_resolve_against_the_live_review_geometry() {
    let mut stack = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Stack,
        110,
        CursorLineMode::Row,
        true,
    );
    let header = stack.locate("selection.ts");
    let sixth = stack.locate("item06");
    stack.drag(
        (header.0.saturating_add(2), header.1),
        (sixth.0.saturating_add(4), sixth.1),
    );
    assert!(stack.app.take_clipboard_copy_request().is_some());

    let mut split = Fixture::new(
        dense_selection_changeset(),
        LayoutMode::Split,
        160,
        CursorLineMode::Row,
        true,
    );
    let first = split.locate("item01");
    let third = split.locate("item03");
    split.drag(
        (first.0.saturating_add(2), first.1),
        (third.0.saturating_add(2), third.1),
    );
    let copied = split.app.take_clipboard_copy_request().unwrap();
    assert!(!copied.is_empty());
    assert!(!copied.contains("1000"), "left-side copy: {copied:?}");
}
