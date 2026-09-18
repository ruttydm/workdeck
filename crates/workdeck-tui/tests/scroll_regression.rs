use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use workdeck_core::ChangesetSource;
use workdeck_diff::parse_patch;
use workdeck_review::LayoutMode;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

fn split_scroll_regression_app() -> ReviewApp {
    let changeset = parse_patch(
        "diff --git a/big.ts b/big.ts\n--- a/big.ts\n+++ b/big.ts\n@@ -33,7 +33,7 @@\n line 33 old value\n line 34 old value\n line 35 old value\n-line 36 old value\n+line 36 new value with long long text abcdefghijklmnopqrstuvwxyz\n line 37 old value\n line 38 old value\n line 39 old value\n",
        "scroll-regression",
        "Working tree",
        ChangesetSource::WorkingTree { staged: false },
    )
    .unwrap();
    ReviewApp::new(
        changeset,
        ReviewOptions {
            layout: LayoutMode::Split,
            ..ReviewOptions::default()
        },
    )
}

fn rendered_review_text(terminal: &mut Terminal<TestBackend>, app: &ReviewApp) -> String {
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), app))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn split_diff_cells_remain_intact_after_a_wheel_scroll_repaint() {
    let backend = TestBackend::new(160, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = split_scroll_regression_app();
    let initial = rendered_review_text(&mut terminal, &app);
    assert!(initial.contains("36 - line 36 old value"), "{initial}");
    assert!(
        initial.contains("36 + line 36 new value with long long te"),
        "{initial}"
    );

    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 50,
        row: 10,
        modifiers: KeyModifiers::NONE,
    });
    let scrolled = rendered_review_text(&mut terminal, &app);
    assert!(scrolled.contains("36 - line 36 old value"), "{scrolled}");
    assert!(
        scrolled.contains("36 + line 36 new value with long long te"),
        "{scrolled}"
    );
    assert!(!scrolled.contains("lold value"), "{scrolled}");
    assert!(!scrolled.contains("36 +  with long long te"), "{scrolled}");
}
