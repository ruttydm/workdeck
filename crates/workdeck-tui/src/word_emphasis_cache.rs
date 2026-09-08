//! Bounded renderer-owned memoization of pure, text-dependent word emphasis.

use std::hash::{DefaultHasher, Hash, Hasher};

use workdeck_diff::{WordDiffRanges, word_diff_ranges};

const SLOTS: usize = 4096;
const MAX_ENTRY_BYTES: usize = 1024;

#[derive(Debug)]
struct Entry {
    old: String,
    new: String,
    ranges: WordDiffRanges,
}

#[derive(Debug, Default)]
pub(crate) struct WordEmphasisCache {
    slots: Vec<Option<Entry>>,
}

impl WordEmphasisCache {
    pub(crate) fn ranges(&mut self, old: &str, new: &str) -> WordDiffRanges {
        let mut hash = DefaultHasher::new();
        (old, new).hash(&mut hash);
        self.ranges_in_slot(old, new, hash.finish() as usize % SLOTS)
    }

    fn ranges_in_slot(&mut self, old: &str, new: &str, slot: usize) -> WordDiffRanges {
        if let Some(Some(entry)) = self.slots.get(slot)
            && entry.old == old
            && entry.new == new
        {
            return entry.ranges.clone();
        }
        let ranges = word_diff_ranges(old, new);
        let bytes = old.len().saturating_add(new.len()).saturating_add(
            (ranges.old.len().saturating_add(ranges.new.len()))
                .saturating_mul(std::mem::size_of::<std::ops::Range<usize>>()),
        );
        // Direct mapping bounds entry count without a recency queue. Oversized
        // results are computed normally and never displace retained entries.
        // Stored strings and range clones have no spare application capacity.
        if bytes <= MAX_ENTRY_BYTES {
            if self.slots.is_empty() {
                self.slots.resize_with(SLOTS, || None);
            }
            self.slots[slot] = Some(Entry {
                old: old.to_owned(),
                new: new.to_owned(),
                ranges: ranges.clone(),
            });
        }
        ranges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_pair_identity_survives_forced_collisions_and_reversal() {
        let mut cache = WordEmphasisCache::default();
        for (old, new) in [
            ("a + b", "a - b"),
            ("日🚀 e\u{301}", "日✨ e"),
            ("a - b", "a + b"),
            ("a + b", "a - b"),
            ("", "new"),
            ("same", "same"),
            ("ab", "c"),
            ("a", "bc"),
        ] {
            let expected = word_diff_ranges(old, new);
            assert_eq!(cache.ranges_in_slot(old, new, 0), expected);
            assert_eq!(cache.ranges_in_slot(old, new, 0), expected);
            assert_eq!(cache.ranges(old, new), expected);
        }
    }

    #[test]
    fn oversized_inputs_bypass_storage_without_evicting_or_changing_results() {
        let mut cache = WordEmphasisCache::default();
        cache.ranges_in_slot("old", "new", 0);
        let old = "x".repeat(MAX_ENTRY_BYTES + 1);
        assert_eq!(
            cache.ranges_in_slot(&old, "", 0),
            word_diff_ranges(&old, "")
        );
        assert_eq!(cache.slots[0].as_ref().unwrap().old, "old");
        assert_eq!(cache.slots.len(), SLOTS);
        let mut fresh = WordEmphasisCache::default();
        fresh.ranges(&old, "");
        assert!(fresh.slots.is_empty());
    }

    #[test]
    fn range_storage_counts_toward_the_entry_payload_limit() {
        let old = "a  ".repeat(100);
        let new = "b  ".repeat(100);
        assert!(old.len() + new.len() < MAX_ENTRY_BYTES);
        let expected = word_diff_ranges(&old, &new);
        assert!(
            old.len()
                + new.len()
                + (expected.old.len() + expected.new.len())
                    * std::mem::size_of::<std::ops::Range<usize>>()
                > MAX_ENTRY_BYTES
        );
        let mut cache = WordEmphasisCache::default();
        assert_eq!(cache.ranges(&old, &new), expected);
        assert!(cache.slots.is_empty());
    }
}
