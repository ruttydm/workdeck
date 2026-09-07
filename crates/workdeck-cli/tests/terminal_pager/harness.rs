//! Incremental Rust translation of Hunk's MIT-licensed `test/pty/harness.ts`.
//! The source file remains unmapped until all helpers and fixture factories are covered.

use super::{Duration, Session};

/// Match the source harness's five interpolated, zero-based mouse positions.
fn drag_positions(start: (usize, usize), end: (usize, usize)) -> [(usize, usize); 5] {
    std::array::from_fn(|index| {
        let interpolate = |start: usize, end: usize| {
            // Coordinates are nonnegative; floor(x + 0.5) matches Math.round.
            (start as f64 + (end as f64 - start as f64) * (index + 1) as f64 / 5.0 + 0.5).floor()
                as usize
        };
        (interpolate(start.0, end.0), interpolate(start.1, end.1))
    })
}

pub(super) fn drag_mouse(session: &mut Session, start: (usize, usize), end: (usize, usize)) {
    session.write(format!("\x1b[<0;{};{}M", start.0 + 1, start.1 + 1).as_bytes());
    session.wait_for(Duration::from_millis(10), |_| false);
    for (x, y) in drag_positions(start, end) {
        session.write(format!("\x1b[<32;{};{}M", x + 1, y + 1).as_bytes());
        session.wait_for(Duration::from_millis(10), |_| false);
    }
    session.write(format!("\x1b[<0;{};{}m", end.0 + 1, end.1 + 1).as_bytes());
    session.wait_for(Duration::from_millis(60), |_| false);
}

#[test]
fn mouse_drag_interpolates_all_five_source_steps_in_both_directions() {
    assert_eq!(
        drag_positions((8, 6), (28, 11)),
        [(12, 7), (16, 8), (20, 9), (24, 10), (28, 11)]
    );
    assert_eq!(
        drag_positions((28, 11), (8, 6)),
        [(24, 10), (20, 9), (16, 8), (12, 7), (8, 6)]
    );
    assert_eq!(
        drag_positions((1, 1), (3, 0)),
        [(1, 1), (2, 1), (2, 0), (3, 0), (3, 0)]
    );
    assert_eq!(drag_positions((0, 0), (0, 0)), [(0, 0); 5]);
}
