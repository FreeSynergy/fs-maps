//! On-disk tile cache with a hard size limit and LRU eviction.
//!
//! Pattern: Decorator — [`CachedSource`] wraps any `TileSource` with a [`TileCache`]; the cache
//! itself is split into config, statistics, an in-memory LRU index and the disk layout.
//!
//! Cache rules (RESEARCH.md): PMTiles tiles are immutable per archive version, so there are no
//! per-tile HTTP caching headers to honour; we apply the OSMF minimum of **7 days** as default
//! `ttl` (an expired entry is refetched). Nothing here ever sends `no-cache` headers.

mod cached;
mod config;
mod diagnostics;
mod disk;
mod index;
mod maintenance;
mod stats;
mod tile_cache;

pub use cached::CachedSource;
pub use config::CacheConfig;
pub use disk::source_hash;
pub use stats::CacheStats;
pub use tile_cache::TileCache;
