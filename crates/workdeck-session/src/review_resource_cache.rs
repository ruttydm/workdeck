//! Daemon-wide LRU and in-flight budget for producer-owned review resources.

use std::fmt;
use std::sync::Arc;

pub const MAX_REVIEW_DAEMON_CACHE_BYTES: usize = 64 * 1_024 * 1_024;
pub const MAX_REVIEW_DAEMON_INFLIGHT_BYTES: usize = 32 * 1_024 * 1_024;
pub const MAX_REVIEW_DAEMON_INFLIGHT_RESOURCES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewResourceCacheLimits {
    pub cache_bytes: usize,
    pub in_flight_bytes: usize,
    pub in_flight_resources: usize,
}

impl Default for ReviewResourceCacheLimits {
    fn default() -> Self {
        Self {
            cache_bytes: MAX_REVIEW_DAEMON_CACHE_BYTES,
            in_flight_bytes: MAX_REVIEW_DAEMON_INFLIGHT_BYTES,
            in_flight_resources: MAX_REVIEW_DAEMON_INFLIGHT_RESOURCES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewResourceKey {
    pub session_id: String,
    pub generation: String,
    pub resource_id: String,
}

/// One accepted reservation. Its opaque token preserves JavaScript object-identity semantics.
#[derive(Debug, PartialEq, Eq)]
pub struct ReviewResourceReservation {
    pub key: ReviewResourceKey,
    pub byte_length: usize,
    token: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewResourceBudgetError {
    message: String,
}

impl ReviewResourceBudgetError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ReviewResourceBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ReviewResourceBudgetError {}

#[derive(Debug)]
struct CacheEntry {
    key: ReviewResourceKey,
    bytes: Arc<[u8]>,
}

#[derive(Debug)]
struct ActiveReservation {
    key: ReviewResourceKey,
    byte_length: usize,
    token: u128,
}

/// Bounded completed bytes in LRU order plus bounded assemblies in progress.
#[derive(Debug)]
pub struct ReviewResourceCache {
    limits: ReviewResourceCacheLimits,
    entries: Vec<CacheEntry>,
    reservations: Vec<ActiveReservation>,
    cached_bytes: usize,
    reserved_bytes: usize,
    next_token: u128,
}

impl Default for ReviewResourceCache {
    fn default() -> Self {
        Self::new(ReviewResourceCacheLimits::default())
    }
}

impl ReviewResourceCache {
    #[must_use]
    pub const fn new(limits: ReviewResourceCacheLimits) -> Self {
        Self {
            limits,
            entries: Vec::new(),
            reservations: Vec::new(),
            cached_bytes: 0,
            reserved_bytes: 0,
            next_token: 1,
        }
    }

    /// Return one completed resource and promote it to the young end of the LRU.
    pub fn get(&mut self, key: &ReviewResourceKey) -> Option<Arc<[u8]>> {
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        let entry = self.entries.remove(index);
        let bytes = Arc::clone(&entry.bytes);
        self.entries.push(entry);
        Some(bytes)
    }

    /// Take one aggregate in-flight slot before an assembly starts.
    pub fn reserve(
        &mut self,
        key: ReviewResourceKey,
        byte_length: usize,
    ) -> Result<ReviewResourceReservation, ReviewResourceBudgetError> {
        if self
            .reservations
            .iter()
            .any(|reservation| reservation.key == key)
        {
            return Err(ReviewResourceBudgetError::new(format!(
                "Review resource {} is already being loaded.",
                key.resource_id
            )));
        }
        if self.reservations.len() >= self.limits.in_flight_resources {
            return Err(ReviewResourceBudgetError::new(format!(
                "The daemon is already assembling {} review resources.",
                self.limits.in_flight_resources
            )));
        }
        if byte_length > self.limits.in_flight_bytes {
            return Err(ReviewResourceBudgetError::new(format!(
                "Review resource {} declares {byte_length} bytes, over the daemon's in-flight budget.",
                key.resource_id
            )));
        }
        if self
            .reserved_bytes
            .checked_add(byte_length)
            .is_none_or(|total| total > self.limits.in_flight_bytes)
        {
            return Err(ReviewResourceBudgetError::new(
                "Review resource loads already fill the daemon's in-flight budget.",
            ));
        }
        let token = self.next_token;
        self.next_token = self.next_token.checked_add(1).ok_or_else(|| {
            ReviewResourceBudgetError::new("Review resource reservation identities are exhausted.")
        })?;
        self.reservations.push(ActiveReservation {
            key: key.clone(),
            byte_length,
            token,
        });
        self.reserved_bytes += byte_length;
        Ok(ReviewResourceReservation {
            key,
            byte_length,
            token,
        })
    }

    /// Resize one active reservation when the producer declares its actual content size.
    pub fn resize(
        &mut self,
        reservation: &mut ReviewResourceReservation,
        byte_length: usize,
    ) -> Result<(), ReviewResourceBudgetError> {
        let Some(active) = self
            .reservations
            .iter_mut()
            .find(|active| active.key == reservation.key && active.token == reservation.token)
        else {
            return Ok(());
        };
        if byte_length > active.byte_length {
            let growth = byte_length - active.byte_length;
            if self
                .reserved_bytes
                .checked_add(growth)
                .is_none_or(|total| total > self.limits.in_flight_bytes)
            {
                return Err(ReviewResourceBudgetError::new(format!(
                    "Review resource {} declares {byte_length} bytes, over the daemon's remaining in-flight budget.",
                    reservation.key.resource_id
                )));
            }
            self.reserved_bytes += growth;
        } else {
            self.reserved_bytes -= active.byte_length - byte_length;
        }
        active.byte_length = byte_length;
        reservation.byte_length = byte_length;
        Ok(())
    }

    /// Give a reservation back exactly once, whether its assembly succeeded or failed.
    pub fn release(&mut self, reservation: &ReviewResourceReservation) {
        let Some(index) = self
            .reservations
            .iter()
            .position(|active| active.key == reservation.key && active.token == reservation.token)
        else {
            return;
        };
        let active = self.reservations.remove(index);
        self.reserved_bytes -= active.byte_length;
    }

    /// Admit verified bytes, evicting oldest entries while never evicting the new value itself.
    pub fn store(&mut self, key: ReviewResourceKey, bytes: Arc<[u8]>) {
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let existing = self.entries.remove(index);
            self.cached_bytes -= existing.bytes.len();
        }
        self.cached_bytes += bytes.len();
        self.entries.push(CacheEntry { key, bytes });
        while self.cached_bytes > self.limits.cache_bytes && self.entries.len() > 1 {
            let oldest = self.entries.remove(0);
            self.cached_bytes -= oldest.bytes.len();
        }
    }

    /// Drop completed and in-flight values belonging to one retired generation.
    pub fn evict_generation(&mut self, session_id: &str, generation: &str) {
        self.evict_where(|key| key.session_id == session_id && key.generation == generation);
    }

    /// Drop completed and in-flight values belonging to one departed session.
    pub fn evict_session(&mut self, session_id: &str) {
        self.evict_where(|key| key.session_id == session_id);
    }

    /// Drop all retained and in-flight values on daemon shutdown.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.reservations.clear();
        self.cached_bytes = 0;
        self.reserved_bytes = 0;
    }

    #[must_use]
    pub const fn cached_bytes(&self) -> usize {
        self.cached_bytes
    }

    #[must_use]
    pub const fn reserved_bytes(&self) -> usize {
        self.reserved_bytes
    }

    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    fn evict_where(&mut self, mut matches: impl FnMut(&ReviewResourceKey) -> bool) {
        let mut index = 0;
        while index < self.entries.len() {
            if matches(&self.entries[index].key) {
                let entry = self.entries.remove(index);
                self.cached_bytes -= entry.bytes.len();
            } else {
                index += 1;
            }
        }
        let mut index = 0;
        while index < self.reservations.len() {
            if matches(&self.reservations[index].key) {
                let reservation = self.reservations.remove(index);
                self.reserved_bytes -= reservation.byte_length;
            } else {
                index += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(resource_id: &str) -> ReviewResourceKey {
        ReviewResourceKey {
            session_id: "s-1".into(),
            generation: "generation:p1:0".into(),
            resource_id: resource_id.into(),
        }
    }

    fn bytes(length: usize) -> Arc<[u8]> {
        vec![0; length].into()
    }

    fn limits() -> ReviewResourceCacheLimits {
        ReviewResourceCacheLimits::default()
    }

    #[test]
    fn returns_what_it_stored() {
        let mut cache = ReviewResourceCache::default();
        let first = key("resource:patch:file:a");
        cache.store(first.clone(), bytes(4));
        assert_eq!(cache.get(&first).unwrap().as_ref(), &[0; 4]);
        assert_eq!(cache.get(&key("resource:patch:file:b")), None);
        assert_eq!(cache.cached_bytes(), 4);
    }

    #[test]
    fn evicts_least_recently_used_entry_to_stay_inside_byte_budget() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            cache_bytes: 10,
            ..limits()
        });
        let first = key("resource:patch:file:a");
        let other = key("resource:patch:file:b");
        cache.store(first.clone(), bytes(6));
        cache.store(other.clone(), bytes(6));
        assert_eq!(cache.get(&first), None);
        assert_eq!(cache.get(&other).unwrap().as_ref(), &[0; 6]);
        assert_eq!(cache.cached_bytes(), 6);
    }

    #[test]
    fn promotes_a_read_entry_before_the_next_eviction() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            cache_bytes: 10,
            ..limits()
        });
        let first = key("resource:patch:file:a");
        let other = key("resource:patch:file:b");
        cache.store(first.clone(), bytes(4));
        cache.store(other.clone(), bytes(4));
        cache.get(&first);
        cache.store(key("resource:patch:file:c"), bytes(4));
        assert!(cache.get(&first).is_some());
        assert_eq!(cache.get(&other), None);
    }

    #[test]
    fn refuses_a_reservation_exceeding_the_inflight_byte_budget() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            in_flight_bytes: 10,
            ..limits()
        });
        cache.reserve(key("resource:patch:file:a"), 8).unwrap();
        assert!(cache.reserve(key("resource:patch:file:b"), 8).is_err());
        assert_eq!(cache.reserved_bytes(), 8);
    }

    #[test]
    fn refuses_more_concurrent_assemblies_than_the_limit() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            in_flight_resources: 1,
            ..limits()
        });
        cache.reserve(key("resource:patch:file:a"), 1).unwrap();
        assert!(cache.reserve(key("resource:patch:file:b"), 1).is_err());
    }

    #[test]
    fn refuses_a_second_reservation_for_the_same_resource() {
        let mut cache = ReviewResourceCache::default();
        let first = key("resource:patch:file:a");
        cache.reserve(first.clone(), 1).unwrap();
        assert!(cache.reserve(first, 1).is_err());
    }

    #[test]
    fn gives_the_budget_back_when_a_load_settles() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            in_flight_bytes: 10,
            ..limits()
        });
        let reservation = cache.reserve(key("resource:patch:file:a"), 8).unwrap();
        cache.release(&reservation);
        assert_eq!(cache.reserved_bytes(), 0);
        assert!(cache.reserve(key("resource:patch:file:b"), 8).is_ok());
    }

    #[test]
    fn resizes_a_reservation_to_the_declared_writer_size() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            in_flight_bytes: 10,
            ..limits()
        });
        let mut reservation = cache.reserve(key("resource:patch:file:a"), 2).unwrap();
        cache.resize(&mut reservation, 9).unwrap();
        assert_eq!(cache.reserved_bytes(), 9);
        assert!(cache.resize(&mut reservation, 11).is_err());
        cache.release(&reservation);
        assert_eq!(cache.reserved_bytes(), 0);
    }

    #[test]
    fn drops_everything_belonging_to_a_retired_generation() {
        let mut cache = ReviewResourceCache::default();
        let first = key("resource:patch:file:a");
        let other = key("resource:patch:file:b");
        cache.store(first.clone(), bytes(4));
        let reservation = cache.reserve(other, 4).unwrap();
        let next = ReviewResourceKey {
            generation: "generation:p1:1".into(),
            ..first.clone()
        };
        cache.store(next.clone(), bytes(4));
        cache.evict_generation("s-1", "generation:p1:0");
        assert_eq!(cache.get(&first), None);
        assert_eq!(cache.reserved_bytes(), 0);
        assert!(cache.get(&next).is_some());
        cache.release(&reservation);
        assert_eq!(cache.reserved_bytes(), 0);
    }

    #[test]
    fn drops_everything_belonging_to_a_departed_session() {
        let mut cache = ReviewResourceCache::default();
        let first = key("resource:patch:file:a");
        let second = ReviewResourceKey {
            session_id: "s-2".into(),
            ..first.clone()
        };
        cache.store(first, bytes(4));
        cache.store(second.clone(), bytes(4));
        cache.evict_session("s-1");
        assert_eq!(cache.entry_count(), 1);
        assert!(cache.get(&second).is_some());
    }

    #[test]
    fn clears_completely_on_shutdown() {
        let mut cache = ReviewResourceCache::default();
        cache.store(key("resource:patch:file:a"), bytes(4));
        cache.reserve(key("resource:patch:file:b"), 4).unwrap();
        cache.clear();
        assert_eq!(cache.cached_bytes(), 0);
        assert_eq!(cache.reserved_bytes(), 0);
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn replacing_an_entry_updates_accounting_and_keeps_the_new_entry() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            cache_bytes: 2,
            ..limits()
        });
        let first = key("resource:patch:file:a");
        cache.store(first.clone(), bytes(1));
        cache.store(first.clone(), bytes(4));
        assert_eq!(cache.cached_bytes(), 4);
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.get(&first).unwrap().len(), 4);
    }

    #[test]
    fn resizing_down_releases_bytes_and_stale_reservations_are_noops() {
        let mut cache = ReviewResourceCache::new(ReviewResourceCacheLimits {
            in_flight_bytes: 10,
            ..limits()
        });
        let mut reservation = cache.reserve(key("resource:patch:file:a"), 8).unwrap();
        cache.resize(&mut reservation, 3).unwrap();
        assert_eq!(cache.reserved_bytes(), 3);
        cache.evict_session("s-1");
        cache.resize(&mut reservation, 9).unwrap();
        cache.release(&reservation);
        assert_eq!(cache.reserved_bytes(), 0);
    }
}
