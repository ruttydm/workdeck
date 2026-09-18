//! Ordinal navigation without materializing or walking the intervening rows.
use std::ops::Range;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct VirtualViewport {
    total: u64,
    selected: Option<u64>,
    offset: u64,
    rows: u64,
}

impl VirtualViewport {
    pub fn new(total: u64, selected: Option<u64>, offset: u64, rows: u64) -> Self {
        let mut viewport = Self {
            total,
            selected,
            offset,
            rows,
        };
        viewport.reveal();
        viewport
    }

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The caller resolves the old qualified selection in the new immutable
    /// query. An absent result starts at the first row, never an unrelated old
    /// ordinal that merely happens to exist in the new ordering.
    pub fn replace(&mut self, total: u64, located: Option<u64>) {
        self.total = total;
        self.selected = located.or((total > 0).then_some(0));
        self.reveal();
    }

    pub fn resize(&mut self, rows: u64) {
        self.rows = rows;
        self.reveal();
    }
    pub fn home(&mut self) {
        self.select(0);
    }
    pub fn end(&mut self) {
        self.select(self.total.saturating_sub(1));
    }
    pub fn select(&mut self, ordinal: u64) {
        self.selected = (self.total > 0).then_some(ordinal.min(self.total.saturating_sub(1)));
        self.reveal();
    }
    pub fn move_by(&mut self, delta: i64) {
        self.select(self.selected.unwrap_or(0).saturating_add_signed(delta));
    }
    pub fn page(&mut self, forward: bool) {
        let selected = self.selected.unwrap_or(0);
        self.select(if forward {
            selected.saturating_add(self.rows.max(1))
        } else {
            selected.saturating_sub(self.rows.max(1))
        });
    }

    pub fn visible(&self) -> Range<u64> {
        self.offset..self.offset.saturating_add(self.rows).min(self.total)
    }

    /// At most eight extra rows per side; callers cannot accidentally turn
    /// overscan into a complete query materialization.
    pub fn requested(&self, overscan: u64) -> Range<u64> {
        let visible = self.visible();
        if visible.is_empty() {
            return visible;
        }
        let extra = overscan.min(8);
        visible.start.saturating_sub(extra)..visible.end.saturating_add(extra).min(self.total)
    }

    fn reveal(&mut self) {
        if self.total == 0 {
            self.selected = None;
            self.offset = 0;
            return;
        }
        self.selected = self.selected.map(|value| value.min(self.total - 1));
        self.offset = self.offset.min(self.total.saturating_sub(self.rows.max(1)));
        if let Some(selected) = self.selected {
            if selected < self.offset {
                self.offset = selected;
            } else if selected >= self.offset.saturating_add(self.rows.max(1)) {
                self.offset = selected.saturating_sub(self.rows.max(1) - 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_queries_jump_directly_and_materialize_only_the_viewport() {
        for total in [10_000, 40_000, u64::MAX] {
            let mut view = VirtualViewport::new(total, Some(0), 0, 12);
            view.end();
            assert_eq!(view.selected(), Some(total - 1));
            assert_eq!(view.visible(), total - 12..total);
            assert!(view.requested(u64::MAX).count() <= 28);
            view.home();
            assert_eq!(view.visible(), 0..12);
            view.select(total / 2);
            assert!(view.visible().contains(&(total / 2)));
            assert_eq!(view.visible().count(), 12);
        }
    }

    #[test]
    fn locate_replaces_ordinals_and_absent_selection_does_not_adopt_an_old_row() {
        let mut view = VirtualViewport::new(40_000, Some(38_000), 37_999, 5);
        view.replace(10_000, Some(17));
        assert_eq!(view.selected(), Some(17));
        assert!(view.visible().contains(&17));
        view.replace(10_000, None);
        assert_eq!(view.selected(), Some(0));
        assert_eq!(view.visible(), 0..5);
        view.replace(0, None);
        assert_eq!(view.selected(), None);
        assert_eq!(view.requested(8), 0..0);
    }

    #[test]
    fn resizing_hidden_views_and_extreme_navigation_keep_selection_reachable() {
        let mut view = VirtualViewport::new(40_000, Some(39_999), 0, 0);
        assert!(view.visible().is_empty());
        assert!(view.requested(8).is_empty());
        view.resize(3);
        assert_eq!(view.visible(), 39_997..40_000);
        view.page(false);
        assert_eq!(view.selected(), Some(39_996));
        view.page(true);
        assert_eq!(view.selected(), Some(39_999));
        view.move_by(i64::MIN);
        assert_eq!(view.selected(), Some(0));
        view.move_by(i64::MAX);
        assert_eq!(view.selected(), Some(39_999));
        view.resize(50_000);
        assert_eq!(view.visible(), 0..40_000);
    }
}
