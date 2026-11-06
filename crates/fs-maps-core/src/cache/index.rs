//! In-memory LRU index of the cache.
//!
//! Pattern: Repository (internal) — keeps key -> entry and the recency order consistent; the
//! disk files are managed by `disk.rs`.

use std::collections::{BTreeMap, HashMap};

use super::stats::CacheStats;

/// `(source hash, z, x, y)`.
pub(super) type Key = (u64, u8, u32, u32);

pub(super) struct Entry {
    pub size: u64,
    pub tick: u64,
    pub stored_at: u64,
}

pub(super) struct Inner {
    pub map: HashMap<Key, Entry>,
    pub order: BTreeMap<u64, Key>,
    pub total: u64,
    pub tick: u64,
    pub stats: CacheStats,
}

impl Inner {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: BTreeMap::new(),
            total: 0,
            tick: 0,
            stats: CacheStats::default(),
        }
    }

    pub fn insert(&mut self, k: Key, e: Entry) {
        self.total += e.size;
        self.order.insert(e.tick, k);
        self.map.insert(k, e);
    }

    pub fn remove(&mut self, k: &Key) -> Option<Entry> {
        let e = self.map.remove(k)?;
        self.order.remove(&e.tick);
        self.total -= e.size;
        Some(e)
    }

    /// Mark `k` as most recently used.
    pub fn touch(&mut self, k: Key) {
        let old = self.map[&k].tick;
        self.order.remove(&old);
        self.tick += 1;
        let t = self.tick;
        self.order.insert(t, k);
        self.map.get_mut(&k).expect("present").tick = t;
    }
}
