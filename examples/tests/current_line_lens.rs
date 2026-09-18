//! Hunk MIT: remaining test/pty/cursor-line.test.ts native extension and mouse cases.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use std::fs;
use workdeck_core::{ChangesetSource, SidebarVisibility};
use workdeck_diff::{create_two_files_patch, parse_patch};
use workdeck_extension_host::LoadedExtension;
use workdeck_review::LayoutMode;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

struct Fixture {
    app: ReviewApp,
    terminal: Terminal<TestBackend>,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn new(before: &str, after: &str, width: u16, height: u16, lens: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let extensions = if lens {
            let bin = directory.path().join("bin");
            fs::create_dir(&bin).unwrap();
            fs::copy(
                env!("CARGO_BIN_EXE_workdeck-example-current-line-lens-extension"),
                bin.join(format!(
                    "workdeck-example-current-line-lens-extension{}",
                    std::env::consts::EXE_SUFFIX
                )),
            )
            .unwrap();
            let manifest = directory.path().join("workdeck-extension.toml");
            fs::write(
                &manifest,
                include_str!("../extensions/current-line-lens/workdeck-extension.toml"),
            )
            .unwrap();
            vec![LoadedExtension::spawn(&manifest, "test").unwrap()]
        } else {
            Vec::new()
        };
        let patch = create_two_files_patch("sample.ts", before, after, 3);
        let changeset = parse_patch(
            &patch,
            "cursor-line",
            "Cursor line",
            ChangesetSource::Files {
                left: "before.ts".into(),
                right: "after.ts".into(),
            },
        )
        .unwrap();
        let mut app = ReviewApp::new_with_extensions(
            changeset,
            ReviewOptions {
                layout: LayoutMode::Split,
                highlight: true,
                sidebar: false,
                sidebar_visibility: SidebarVisibility::Hidden,
                prompt_save_view_preferences: false,
                ..ReviewOptions::default()
            },
            extensions,
        );
        app.set_clipboard_copy_supported(true);
        Self {
            app,
            terminal: Terminal::new(TestBackend::new(width, height)).unwrap(),
            _directory: directory,
        }
    }

    fn draw(&mut self) -> String {
        self.terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &self.app))
            .unwrap();
        let buffer = self.terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                let mut line = String::new();
                let mut x = 0;
                while x < buffer.area.width {
                    let symbol = buffer[(x, y)].symbol();
                    line.push_str(symbol);
                    x += workdeck_tui::measure_text_width(symbol).max(1) as u16;
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(&mut self, code: KeyCode) -> String {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        self.app.poll_extension_commands();
        self.draw()
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> String {
        self.app.handle_mouse_event(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        self.draw()
    }

    fn drag(&mut self, column: u16, row: u16, end_column: u16) -> String {
        self.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
        self.mouse(MouseEventKind::Drag(MouseButton::Left), end_column, row);
        self.mouse(MouseEventKind::Up(MouseButton::Left), end_column, row)
    }
}

fn row(frame: &str, needle: &str) -> usize {
    frame
        .lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle}: {frame}"))
}

fn numbered(offset: u32) -> String {
    (1..=60)
        .map(|line| format!("export const line{line:02} = {};\n", line + offset))
        .collect()
}

#[test]
fn current_line_pane_pins_old_above_new_and_hides_in_stack() {
    let mut fixture = Fixture::new(
        "export const message = 'short';\n",
        "export const message = 'this is a very long wrapped line';\n",
        140,
        18,
        true,
    );
    let split = fixture.draw();
    let lens = row(&split, "Current line · old above, new below");
    assert!(
        split
            .lines()
            .nth(lens + 1)
            .unwrap()
            .contains("export const message = 'short';")
    );
    assert!(
        split
            .lines()
            .nth(lens + 2)
            .unwrap()
            .contains("this is a very long wrapped line")
    );
    assert!(!fixture.press(KeyCode::Char('2')).contains("Current line ·"));
    assert!(fixture.press(KeyCode::Char('1')).contains("Current line ·"));
}

#[test]
fn stepping_updates_unicode_lens_without_moving_fixed_rectangle() {
    let mut fixture = Fixture::new(
        "const label = '日本語';\nconst equal = true;\nconst plain = 'before';\n",
        "const label = '한국어';\nconst equal = true;\nconst plain = 'after';\n",
        140,
        18,
        true,
    );
    let initial = fixture.draw();
    let lens = row(&initial, "Current line ·");
    assert!(initial.lines().nth(lens + 1).unwrap().contains("日本語"));
    assert!(initial.lines().nth(lens + 2).unwrap().contains("한국어"));
    let mut moved = String::new();
    for _ in 0..4 {
        moved = fixture.press(KeyCode::Char('j'));
    }
    assert_eq!(row(&moved, "Current line ·"), lens);
    assert!(
        moved
            .lines()
            .nth(lens + 1)
            .unwrap()
            .contains("plain = 'before'")
    );
    assert!(
        moved
            .lines()
            .nth(lens + 2)
            .unwrap()
            .contains("plain = 'after'")
    );
}

#[test]
fn one_cell_mouse_jitter_selects_exact_line_before_and_after_paging() {
    let mut fixture = Fixture::new(&numbered(0), &numbered(100), 120, 16, true);
    let initial = fixture.draw();
    let clicked_row = row(&initial, "export const line05 = 5;") as u16;
    let clicked = fixture.drag(30, clicked_row, 31);
    assert!(
        clicked
            .rsplit("Current line")
            .next()
            .unwrap()
            .contains("export const line05 = 5;")
    );
    assert!(!clicked.contains("Copied selection to clipboard"));
    let stepped = fixture.press(KeyCode::Down);
    assert!(
        stepped
            .rsplit("Current line")
            .next()
            .unwrap()
            .contains("export const line05 = 5;")
    );
    let paged = fixture.press(KeyCode::PageDown);
    assert!(!paged.contains("export const line01 = 1;"));
    let next_row = row(&paged, "export const line12 = 12;") as u16;
    let clicked = fixture.drag(30, next_row, 30);
    assert!(
        clicked
            .rsplit("Current line")
            .next()
            .unwrap()
            .contains("export const line12 = 12;")
    );
}

#[test]
fn copy_drag_extends_across_repainted_highlighted_rows() {
    let mut fixture = Fixture::new(&numbered(0), &numbered(100), 120, 20, false);
    let initial = fixture.draw();
    let start = row(&initial, "export const line02 = 2;") as u16;
    let end = row(&initial, "export const line06 = 6;") as u16;
    assert!(end > start + 2);
    let backgrounds = |fixture: &Fixture, y| {
        (0..120)
            .map(|x| fixture.terminal.backend().buffer()[(x, y)].bg)
            .collect::<Vec<_>>()
    };
    let before = (start..=end)
        .map(|y| backgrounds(&fixture, y))
        .collect::<Vec<_>>();
    fixture.mouse(MouseEventKind::Down(MouseButton::Left), 30, start);
    for y in start + 1..=end {
        fixture.mouse(MouseEventKind::Drag(MouseButton::Left), 30, y);
    }
    for (index, y) in (start..=end).enumerate() {
        assert_ne!(backgrounds(&fixture, y), before[index]);
    }
    let released = fixture.mouse(MouseEventKind::Up(MouseButton::Left), 30, end);
    assert!(released.contains("Copied selection to clipboard"));
}

#[test]
fn lens_fixture_retains_fixed_registration_clipped_rule_and_empty_state() {
    use workdeck_examples::current_line_lens_extension::{registration, render as render_lens};
    use workdeck_extension_api::{
        ExtensionCurrentLinePaint, ExtensionFileSide, PanePlacement, PaneRenderRequest, ViewNode,
    };
    let pane = registration();
    assert_eq!(pane.id, "current-line");
    assert_eq!(pane.placement, PanePlacement::Bottom);
    assert!(pane.default_open && pane.current_line && pane.available);
    let height = pane.height.unwrap();
    assert_eq!(
        (height.preferred, height.min, height.max),
        (3, Some(3), Some(3))
    );
    let fixture = Fixture::new("old\n", "new\n", 140, 18, false);
    let theme =
        workdeck_tui::to_extension_paint_theme(&workdeck_tui::resolve_theme(None, None, &[]));
    let mut request = PaneRenderRequest {
        pane_id: pane.id,
        snapshot: fixture.app.shared_state().lock().unwrap().snapshot(),
        placement: PanePlacement::Bottom,
        width: 140,
        height: 3,
        theme: theme.clone(),
        files: Vec::new(),
        selected_file_id: None,
        selected_hunk_index: None,
        current_line: None,
        keybindings: Default::default(),
    };
    assert_eq!(render_lens(&request).content, ViewNode::Empty);
    request.current_line = Some(ExtensionCurrentLinePaint {
        side: ExtensionFileSide::New,
        line: 1,
    });
    for width in [0, 1, 10, 140] {
        request.width = width;
        let ViewNode::Column { children, gap } = render_lens(&request).content else {
            panic!("expected three-row lens")
        };
        assert_eq!(gap, 0);
        assert_eq!(children.len(), 3);
        let ViewNode::Text { text, style } = &children[0] else {
            panic!("missing rule")
        };
        assert_eq!(text.chars().count(), usize::from(width));
        assert_eq!(style.foreground, Some(theme.border.clone()));
        assert_eq!(style.background, Some(theme.panel.clone()));
        assert_eq!(
            children[1],
            ViewNode::CurrentLine {
                side: ExtensionFileSide::Old,
                width
            }
        );
        assert_eq!(
            children[2],
            ViewNode::CurrentLine {
                side: ExtensionFileSide::New,
                width
            }
        );
    }
}

#[test]
fn lens_availability_protocol_requires_a_host_current_line() {
    use workdeck_extension_api::{
        ExtensionCurrentLinePaint, ExtensionFileSide, PaneAvailabilityRequest, PanePlacement,
    };
    let mut input = String::new();
    for (id, current_line) in [
        None,
        Some(ExtensionCurrentLinePaint {
            side: ExtensionFileSide::Old,
            line: 5,
        }),
    ]
    .into_iter()
    .enumerate()
    {
        let request = PaneAvailabilityRequest {
            pane_id: "current-line".into(),
            placement: PanePlacement::Bottom,
            files: Vec::new(),
            selected_file_id: None,
            selected_hunk_index: None,
            current_line,
        };
        input.push_str(&serde_json::json!({"jsonrpc":"2.0", "id":id, "method":"workdeck/pane/available", "params":request}).to_string());
        input.push('\n');
    }
    let mut output = Vec::new();
    workdeck_examples::current_line_lens_extension::serve(std::io::Cursor::new(input), &mut output)
        .unwrap();
    let output = String::from_utf8(output).unwrap();
    let responses = output
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["result"]["available"], false);
    assert_eq!(responses[1]["result"]["available"], true);
}
