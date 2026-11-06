//! Test/diagnostic accessors of the cache.
//!
//! Pattern: Facade (read-only view) — exposes internals for tests without widening the API.

use std::fs;
use std::path::Path;

use super::tile_cache::TileCache;

impl TileCache {
    /// Keys in LRU order, oldest first (tests / diagnostics).
    #[doc(hidden)]
    pub fn keys_oldest_first(&self) -> Vec<(u64, u8, u32, u32)> {
        self.inner
            .lock()
            .expect("cache lock")
            .order
            .values()
            .copied()
            .collect()
    }

    /// Actual bytes on disk under the cache dir (tests).
    #[doc(hidden)]
    pub fn disk_bytes(&self) -> u64 {
        fn walk(p: &Path) -> u64 {
            let Ok(rd) = fs::read_dir(p) else { return 0 };
            rd.flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => walk(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        }
        walk(&self.dir())
    }
}
