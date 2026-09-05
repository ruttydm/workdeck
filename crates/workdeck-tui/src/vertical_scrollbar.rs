//! Auto-hiding vertical review scrollbar ported from Hunk's terminal component.

use std::time::{Duration, Instant};

pub const VERTICAL_SCROLLBAR_HIDE_DELAY: Duration = Duration::from_millis(2_000);
pub const VERTICAL_SCROLLBAR_WIDTH: u16 = 1;
pub const VERTICAL_SCROLLBAR_MIN_THUMB_HEIGHT: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerticalScrollbarGeometry {
    pub track_height: usize,
    pub thumb_height: usize,
    pub max_thumb_y: isize,
    pub max_scroll: usize,
    pub thumb_y: isize,
}

impl VerticalScrollbarGeometry {
    #[must_use]
    pub fn resolve(content_height: usize, viewport_height: usize, scroll_top: usize) -> Self {
        let max_scroll = content_height.saturating_sub(viewport_height);
        let thumb_height = viewport_height
            .saturating_mul(viewport_height)
            .checked_div(content_height)
            .unwrap_or(0)
            .max(VERTICAL_SCROLLBAR_MIN_THUMB_HEIGHT);
        let max_thumb_y = isize::try_from(viewport_height).unwrap_or(isize::MAX)
            - isize::try_from(thumb_height).unwrap_or(isize::MAX);
        let scroll_percent = if max_scroll > 0 {
            scroll_top.min(max_scroll) as f64 / max_scroll as f64
        } else {
            0.0
        };
        let thumb_y = (scroll_percent * max_thumb_y as f64).floor() as isize;
        Self {
            track_height: viewport_height,
            thumb_height,
            max_thumb_y,
            max_scroll,
            thumb_y,
        }
    }

    #[must_use]
    pub fn thumb_contains(self, relative_y: isize) -> bool {
        relative_y >= self.thumb_y
            && relative_y
                < self
                    .thumb_y
                    .saturating_add(isize::try_from(self.thumb_height).unwrap_or(isize::MAX))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerticalScrollbarPresentation {
    pub geometry: VerticalScrollbarGeometry,
    pub dragging: bool,
    pub scroll_top: usize,
}

#[derive(Debug, Clone)]
pub struct VerticalScrollbarController {
    visible: bool,
    dragging: bool,
    drag_start_y: isize,
    drag_start_scroll: f64,
    hide_delay: Duration,
    hide_deadline: Option<Instant>,
    hide_generation: u64,
    observed_scroll_top: Option<usize>,
}

impl Default for VerticalScrollbarController {
    fn default() -> Self {
        Self::new(VERTICAL_SCROLLBAR_HIDE_DELAY)
    }
}

impl VerticalScrollbarController {
    #[must_use]
    pub const fn new(hide_delay: Duration) -> Self {
        Self {
            visible: false,
            dragging: false,
            drag_start_y: 0,
            drag_start_scroll: 0.0,
            hide_delay,
            hide_deadline: None,
            hide_generation: 0,
            observed_scroll_top: None,
        }
    }

    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    #[must_use]
    pub const fn is_dragging(&self) -> bool {
        self.dragging
    }

    #[must_use]
    pub const fn hide_generation(&self) -> u64 {
        self.hide_generation
    }

    #[must_use]
    pub const fn hide_deadline(&self) -> Option<Instant> {
        self.hide_deadline
    }

    /// Reveal for one full inactivity window and supersede the preceding deadline.
    pub fn show(&mut self, now: Instant) {
        self.visible = true;
        self.schedule_hide(now);
    }

    /// Preserve Hunk's optional activity callback without coupling the controller to its owner.
    pub fn show_with_activity(&mut self, now: Instant, on_activity: impl FnOnce()) {
        self.show(now);
        on_activity();
    }

    /// Observe the imperative viewport. Its first value is a baseline rather than user activity.
    pub fn observe_scroll_top(&mut self, scroll_top: usize, now: Instant) {
        match self.observed_scroll_top.replace(scroll_top) {
            Some(previous) if previous != scroll_top => self.show(now),
            _ => {}
        }
    }

    pub fn tick(&mut self, now: Instant) {
        if self.hide_deadline.is_some_and(|deadline| now >= deadline) {
            let generation = self.hide_generation;
            self.fire_hide(generation);
        }
    }

    /// Generation check models a canceled callback that may still be delivered by its scheduler.
    pub fn fire_hide(&mut self, generation: u64) {
        if generation != self.hide_generation {
            return;
        }
        self.hide_deadline = None;
        if !self.dragging {
            self.visible = false;
        }
    }

    pub fn begin_drag(&mut self, relative_y: isize, scroll_top: usize, now: Instant) {
        self.dragging = true;
        self.drag_start_y = relative_y;
        self.drag_start_scroll = scroll_top as f64;
        self.show(now);
    }

    #[must_use]
    pub fn drag_to(
        &mut self,
        relative_y: isize,
        geometry: VerticalScrollbarGeometry,
        now: Instant,
    ) -> Option<f64> {
        if !self.dragging {
            return None;
        }
        let delta_y = relative_y.saturating_sub(self.drag_start_y) as f64;
        let pixels_per_row = if geometry.max_thumb_y > 0 && geometry.max_scroll > 0 {
            geometry.max_thumb_y as f64 / geometry.max_scroll as f64
        } else {
            1.0
        };
        let scroll_top = (self.drag_start_scroll + delta_y / pixels_per_row)
            .clamp(0.0, geometry.max_scroll as f64);
        self.show(now);
        Some(scroll_top)
    }

    pub fn end_drag(&mut self, now: Instant) -> bool {
        if !self.dragging {
            return false;
        }
        self.dragging = false;
        self.schedule_hide(now);
        true
    }

    #[must_use]
    pub fn track_click(
        &mut self,
        relative_y: isize,
        geometry: VerticalScrollbarGeometry,
        scroll_top: usize,
        now: Instant,
    ) -> Option<usize> {
        let next = if relative_y < geometry.thumb_y {
            Some(scroll_top.saturating_sub(geometry.track_height))
        } else if relative_y
            >= geometry
                .thumb_y
                .saturating_add(isize::try_from(geometry.thumb_height).unwrap_or(isize::MAX))
        {
            Some(
                scroll_top
                    .saturating_add(geometry.track_height)
                    .min(geometry.max_scroll),
            )
        } else {
            None
        };
        self.show(now);
        next
    }

    #[must_use]
    pub fn presentation(
        &self,
        content_height: usize,
        viewport_height: usize,
        scroll_top: usize,
    ) -> Option<VerticalScrollbarPresentation> {
        (self.visible && content_height > viewport_height).then(|| VerticalScrollbarPresentation {
            geometry: VerticalScrollbarGeometry::resolve(
                content_height,
                viewport_height,
                scroll_top,
            ),
            dragging: self.dragging,
            scroll_top: scroll_top.min(content_height.saturating_sub(viewport_height)),
        })
    }

    fn schedule_hide(&mut self, now: Instant) {
        self.hide_generation = self.hide_generation.saturating_add(1);
        self.hide_deadline = Some(now + self.hide_delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn overflowing_content_is_revealed_for_one_complete_hide_window() {
        let start = Instant::now();
        let mut scrollbar = VerticalScrollbarController::new(Duration::from_millis(120));
        assert!(scrollbar.presentation(40, 10, 0).is_none());

        scrollbar.show(start);
        let presentation = scrollbar.presentation(40, 10, 0).unwrap();
        assert_eq!(presentation.geometry.thumb_height, 2);
        assert_eq!(presentation.geometry.thumb_y, 0);
        scrollbar.tick(start + Duration::from_millis(119));
        assert!(scrollbar.is_visible());
        scrollbar.tick(start + Duration::from_millis(120));
        assert!(!scrollbar.is_visible());
    }

    #[test]
    fn repeated_activity_restarts_deadline_and_stale_callbacks_do_not_hide() {
        let start = Instant::now();
        let mut scrollbar = VerticalScrollbarController::new(Duration::from_millis(120));
        let activity_calls = Cell::new(0);
        scrollbar.show(start);
        let stale_generation = scrollbar.hide_generation();
        scrollbar.show_with_activity(start + Duration::from_millis(100), || {
            activity_calls.set(activity_calls.get() + 1);
        });
        assert_eq!(activity_calls.get(), 1);
        scrollbar.fire_hide(stale_generation);
        assert!(scrollbar.is_visible());
        scrollbar.tick(start + Duration::from_millis(219));
        assert!(scrollbar.is_visible());
        scrollbar.tick(start + Duration::from_millis(220));
        assert!(!scrollbar.is_visible());
    }

    #[test]
    fn content_that_fits_never_produces_a_presentation() {
        let mut scrollbar = VerticalScrollbarController::default();
        scrollbar.show(Instant::now());
        assert!(scrollbar.presentation(10, 10, 0).is_none());
        assert!(scrollbar.presentation(9, 10, 0).is_none());
    }

    #[test]
    fn dragging_maps_track_cells_to_content_and_restarts_hide_on_release() {
        let start = Instant::now();
        let geometry = VerticalScrollbarGeometry::resolve(100, 10, 0);
        let mut scrollbar = VerticalScrollbarController::new(Duration::from_millis(120));
        scrollbar.begin_drag(0, 0, start);
        assert_eq!(scrollbar.drag_to(4, geometry, start).unwrap(), 45.0);
        let drag_generation = scrollbar.hide_generation();
        scrollbar.tick(start + Duration::from_millis(120));
        assert!(scrollbar.is_visible());
        assert!(scrollbar.hide_deadline().is_none());
        assert!(scrollbar.end_drag(start + Duration::from_millis(120)));
        assert!(scrollbar.hide_generation() > drag_generation);
        scrollbar.tick(start + Duration::from_millis(239));
        assert!(scrollbar.is_visible());
        scrollbar.tick(start + Duration::from_millis(240));
        assert!(!scrollbar.is_visible());
    }

    #[test]
    fn track_click_pages_one_viewport_above_or_below_the_thumb() {
        let now = Instant::now();
        let geometry = VerticalScrollbarGeometry::resolve(100, 10, 20);
        let mut scrollbar = VerticalScrollbarController::default();
        assert_eq!(scrollbar.track_click(8, geometry, 20, now), Some(30));
        assert_eq!(scrollbar.track_click(0, geometry, 20, now), Some(10));
        assert_eq!(
            scrollbar.track_click(geometry.thumb_y, geometry, 20, now),
            None
        );
    }

    #[test]
    fn barely_overflowing_content_clamps_a_large_drag_to_the_only_row() {
        let now = Instant::now();
        let geometry = VerticalScrollbarGeometry::resolve(11, 10, 0);
        assert_eq!(geometry.thumb_height, 9);
        assert_eq!(geometry.max_thumb_y, 1);
        let mut scrollbar = VerticalScrollbarController::default();
        scrollbar.begin_drag(0, 0, now);
        assert_eq!(scrollbar.drag_to(5, geometry, now), Some(1.0));
    }

    #[test]
    fn first_viewport_sample_is_baseline_and_later_movement_is_activity() {
        let now = Instant::now();
        let mut scrollbar = VerticalScrollbarController::default();
        scrollbar.observe_scroll_top(4, now);
        assert!(!scrollbar.is_visible());
        scrollbar.observe_scroll_top(4, now);
        assert!(!scrollbar.is_visible());
        scrollbar.observe_scroll_top(5, now);
        assert!(scrollbar.is_visible());
    }
}
