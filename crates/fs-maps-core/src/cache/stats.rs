//! Cache statistics snapshot.
//!
//! Pattern: Value object — a plain copyable snapshot, produced by `TileCache::stats`.

/// Counters and sizes of a cache at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Entries currently stored.
    pub entries: u64,
    /// Bytes currently stored (headers included).
    pub bytes: u64,
    /// Configured limit.
    pub max_bytes: u64,
    /// Lookups answered from the cache.
    pub hits: u64,
    /// Lookups not answered.
    pub misses: u64,
    /// Entries removed to honour the limit.
    pub evictions: u64,
    /// Entries removed because their TTL passed.
    pub expired: u64,
}
