//! Burst-sensitive review mouse-wheel acceleration.
//!
//! The curve and timing constants translate OpenTUI 0.5.6's MIT-licensed
//! `MacOSScrollAccel`, configured with the exact values selected by Hunk.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const ACCELERATION: f64 = 0.4;
const TIME_CONSTANT: f64 = 4.0;
const MAX_MULTIPLIER: f64 = 3.0;
const HISTORY_SIZE: usize = 3;
const STREAK_TIMEOUT: Duration = Duration::from_millis(150);
const MIN_TICK_INTERVAL: Duration = Duration::from_millis(6);
const REFERENCE_INTERVAL_MS: f64 = 100.0;

/// Keeps the first wheel tick precise, then ramps sustained gestures.
#[derive(Debug, Default)]
pub struct ReviewMouseWheelScrollAcceleration {
    last_tick: Option<Instant>,
    velocity_history_ms: VecDeque<f64>,
}

impl ReviewMouseWheelScrollAcceleration {
    #[must_use]
    pub fn tick(&mut self, now: Instant) -> f64 {
        let Some(last_tick) = self.last_tick else {
            self.last_tick = Some(now);
            self.velocity_history_ms.clear();
            return 1.0;
        };
        let interval = now.saturating_duration_since(last_tick);
        if interval > STREAK_TIMEOUT {
            self.last_tick = Some(now);
            self.velocity_history_ms.clear();
            return 1.0;
        }
        if interval < MIN_TICK_INTERVAL {
            return 1.0;
        }
        self.last_tick = Some(now);
        self.velocity_history_ms
            .push_back(interval.as_secs_f64() * 1_000.0);
        if self.velocity_history_ms.len() > HISTORY_SIZE {
            self.velocity_history_ms.pop_front();
        }
        let average_interval =
            self.velocity_history_ms.iter().sum::<f64>() / self.velocity_history_ms.len() as f64;
        let velocity = REFERENCE_INTERVAL_MS / average_interval;
        let multiplier = 1.0 + ACCELERATION * ((velocity / TIME_CONSTANT).exp() - 1.0);
        multiplier.min(MAX_MULTIPLIER)
    }

    pub fn reset(&mut self) {
        self.last_tick = None;
        self.velocity_history_ms.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_tick_is_precise_and_a_new_streak_resets() {
        let start = Instant::now();
        let mut acceleration = ReviewMouseWheelScrollAcceleration::default();
        assert_eq!(acceleration.tick(start), 1.0);
        assert_eq!(acceleration.tick(start + Duration::from_millis(151)), 1.0);
    }

    #[test]
    fn sustained_ticks_accelerate_with_hunks_exact_curve_and_cap() {
        let start = Instant::now();
        let mut acceleration = ReviewMouseWheelScrollAcceleration::default();
        assert_eq!(acceleration.tick(start), 1.0);
        let at_50_ms = acceleration.tick(start + Duration::from_millis(50));
        let expected = 1.0 + 0.4 * ((2.0_f64 / 4.0).exp() - 1.0);
        assert!((at_50_ms - expected).abs() < f64::EPSILON * 8.0);

        let capped = acceleration.tick(start + Duration::from_millis(56));
        assert!(capped > at_50_ms);
        for offset in [62, 68, 74] {
            assert!(acceleration.tick(start + Duration::from_millis(offset)) <= 3.0);
        }
    }

    #[test]
    fn sub_six_millisecond_ticks_do_not_advance_the_streak_clock() {
        let start = Instant::now();
        let mut acceleration = ReviewMouseWheelScrollAcceleration::default();
        let _ = acceleration.tick(start);
        assert_eq!(acceleration.tick(start + Duration::from_millis(5)), 1.0);
        let multiplier = acceleration.tick(start + Duration::from_millis(10));
        let expected = 1.0 + 0.4 * ((10.0_f64 / 4.0).exp() - 1.0);
        assert_eq!(multiplier, expected.min(3.0));
    }

    #[test]
    fn reset_makes_the_next_tick_precise_again() {
        let start = Instant::now();
        let mut acceleration = ReviewMouseWheelScrollAcceleration::default();
        let _ = acceleration.tick(start);
        assert!(acceleration.tick(start + Duration::from_millis(10)) > 1.0);
        acceleration.reset();
        assert_eq!(acceleration.tick(start + Duration::from_millis(20)), 1.0);
    }
}
