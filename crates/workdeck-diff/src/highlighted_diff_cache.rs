//! Line-budgeted UI highlight cache translated from Hunk's
//! `src/ui/diff/highlightedDiffCache.ts`.

use std::collections::{HashMap, VecDeque};

use crate::HighlightedFile;

/// Budget counted in highlighted old- and new-side lines.
pub const MAX_HIGHLIGHTED_DIFF_CACHE_LINES: usize = 60_000;

/// Bookkeeping charged even to empty, skipped, or failed highlight results.
const ENTRY_OVERHEAD_LINES: usize = 8;

#[derive(Debug, Clone)]
struct HighlightedDiffCacheEntry {
    cost: usize,
    value: HighlightedDiffCode,
}

/// Native equivalent of Hunk's side-separated highlighted diff result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedDiffCode {
    pub highlighted: HighlightedFile,
    pub deletion_line_count: usize,
    pub addition_line_count: usize,
}

impl HighlightedDiffCode {
    #[must_use]
    pub const fn new(
        highlighted: HighlightedFile,
        deletion_line_count: usize,
        addition_line_count: usize,
    ) -> Self {
        Self {
            highlighted,
            deletion_line_count,
            addition_line_count,
        }
    }

    #[must_use]
    pub const fn retained_line_count(&self) -> usize {
        self.deletion_line_count
            .saturating_add(self.addition_line_count)
    }
}

/// Bounded least-recently-used cache for terminal-owned highlight results.
#[derive(Debug)]
pub struct HighlightedDiffCache {
    entries: HashMap<String, HighlightedDiffCacheEntry>,
    lru: VecDeque<String>,
    budget: usize,
    cached_cost: usize,
}

impl Default for HighlightedDiffCache {
    fn default() -> Self {
        Self::new(MAX_HIGHLIGHTED_DIFF_CACHE_LINES)
    }
}

impl HighlightedDiffCache {
    #[must_use]
    pub fn new(max_lines: usize) -> Self {
        Self {
            entries: HashMap::new(),
            lru: VecDeque::new(),
            budget: max_lines.max(1),
            cached_cost: 0,
        }
    }

    /// Read one result and mark it most recently used.
    pub fn get(&mut self, key: &str) -> Option<HighlightedDiffCode> {
        let value = self.entries.get(key)?.value.clone();
        self.touch(key);
        Some(value)
    }

    /// Read one result without changing recency.
    #[must_use]
    pub fn peek(&self, key: &str) -> Option<&HighlightedDiffCode> {
        self.entries.get(key).map(|entry| &entry.value)
    }

    /// Store one result as most recently used, evicting least-recently-used entries over budget.
    pub fn set(&mut self, key: String, value: HighlightedDiffCode) {
        if let Some(previous) = self.entries.remove(&key) {
            self.cached_cost = self.cached_cost.saturating_sub(previous.cost);
            self.remove_from_lru(&key);
        }

        let cost = value
            .retained_line_count()
            .saturating_add(ENTRY_OVERHEAD_LINES);
        self.entries
            .insert(key.clone(), HighlightedDiffCacheEntry { cost, value });
        self.lru.push_back(key);
        self.cached_cost = self.cached_cost.saturating_add(cost);

        // Keep one oversized result so the file currently being read does not evict itself.
        while self.cached_cost > self.budget && self.entries.len() > 1 {
            let Some(least_recently_used) = self.lru.pop_front() else {
                return;
            };
            if let Some(evicted) = self.entries.remove(&least_recently_used) {
                self.cached_cost = self.cached_cost.saturating_sub(evicted.cost);
            }
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.cached_cost = 0;
    }

    fn touch(&mut self, key: &str) {
        self.remove_from_lru(key);
        self.lru.push_back(key.to_owned());
    }

    fn remove_from_lru(&mut self, key: &str) {
        if let Some(index) = self.lru.iter().position(|candidate| candidate == key) {
            self.lru.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlighted(lines: usize) -> HighlightedDiffCode {
        HighlightedDiffCode::new(vec![vec![Vec::new(); lines]], lines, 0)
    }

    #[test]
    fn evicts_the_least_recently_used_entry_rather_than_the_oldest_highlight() {
        let mut cache = HighlightedDiffCache::new(40);
        let on_screen = highlighted(10);
        let scrolled_past = highlighted(10);
        let prefetched = highlighted(10);

        cache.set("on-screen".into(), on_screen.clone());
        cache.set("scrolled-past".into(), scrolled_past);
        assert_eq!(cache.get("on-screen"), Some(on_screen.clone()));
        cache.set("prefetched".into(), prefetched.clone());

        assert_eq!(cache.peek("on-screen"), Some(&on_screen));
        assert_eq!(cache.peek("prefetched"), Some(&prefetched));
        assert!(cache.peek("scrolled-past").is_none());
    }

    #[test]
    fn peeking_does_not_protect_an_entry_from_eviction() {
        let mut cache = HighlightedDiffCache::new(20);
        let first = highlighted(10);
        let second = highlighted(10);
        cache.set("first".into(), first.clone());
        assert_eq!(cache.peek("first"), Some(&first));
        cache.set("second".into(), second.clone());
        assert!(cache.peek("first").is_none());
        assert_eq!(cache.peek("second"), Some(&second));
    }

    #[test]
    fn holds_far_more_small_files_than_large_ones_under_the_same_budget() {
        let mut cache = HighlightedDiffCache::new(600);
        for index in 0..50 {
            cache.set(format!("small-{index}"), highlighted(2));
        }
        assert!(cache.peek("small-0").is_some());
        assert!(cache.peek("small-49").is_some());

        for index in 0..50 {
            cache.set(format!("large-{index}"), highlighted(60));
        }
        assert!(cache.peek("large-49").is_some());
        assert!(cache.peek("large-0").is_none());
        assert!(cache.peek("small-0").is_none());
    }

    #[test]
    fn keeps_a_result_larger_than_the_whole_budget_instead_of_dropping_it() {
        let mut cache = HighlightedDiffCache::new(100);
        let generated = highlighted(5_000);
        cache.set("neighbor".into(), highlighted(50));
        cache.set("generated".into(), generated.clone());
        assert_eq!(cache.peek("generated"), Some(&generated));
        assert!(cache.peek("neighbor").is_none());
    }

    #[test]
    fn releases_the_budget_a_replaced_result_was_holding() {
        let mut cache = HighlightedDiffCache::new(100);
        let reloaded = highlighted(4);
        let kept = highlighted(40);
        cache.set("reloaded".into(), highlighted(90));
        cache.set("reloaded".into(), reloaded.clone());
        cache.set("kept".into(), kept.clone());
        assert_eq!(cache.peek("reloaded"), Some(&reloaded));
        assert_eq!(cache.peek("kept"), Some(&kept));
    }

    #[test]
    fn reclaims_entries_that_retain_no_lines_at_all() {
        let mut cache = HighlightedDiffCache::new(100);
        for index in 0..200 {
            cache.set(format!("skipped-{index}"), highlighted(0));
        }
        assert!(cache.peek("skipped-199").is_some());
        assert!(cache.peek("skipped-0").is_none());
    }

    #[test]
    fn keeps_one_entry_when_given_a_degenerate_budget() {
        let mut cache = HighlightedDiffCache::new(0);
        let only = highlighted(5);
        cache.set("only".into(), only.clone());
        assert_eq!(cache.peek("only"), Some(&only));
    }
}
