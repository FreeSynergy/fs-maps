//! Eviction and clearing of the cache.
//!
//! Pattern: Strategy (LRU policy) — drops the least recently used entries until the size limit
//! holds; `clear` drops everything.

use std::fs;

use super::disk::{path_of, read_dir};
use super::index::Inner;
use super::tile_cache::TileCache;
use crate::error::{Error, Result};

impl TileCache {
    pub(super) fn evict(&self, g: &mut Inner) {
        let (dir, max) = {
            let c = self.cfg.lock().expect("cfg lock");
            (c.dir.clone(), c.max_bytes)
        };
        while g.total > max {
            let Some((&t, &k)) = g.order.iter().next() else {
                break;
            };
            g.order.remove(&t);
            if let Some(e) = g.map.remove(&k) {
                g.total -= e.size;
            }
            let _ = fs::remove_file(path_of(&dir, k));
            g.stats.evictions += 1;
        }
    }

    /// Remove everything.
    pub fn clear(&self) -> Result<()> {
        let dir = self.dir();
        let mut g = self.inner.lock().expect("cache lock");
        for src in read_dir(&dir)? {
            if src.path().is_dir() {
                fs::remove_dir_all(src.path()).map_err(|e| Error::io(src.path(), e))?;
            }
        }
        g.map.clear();
        g.order.clear();
        g.total = 0;
        Ok(())
    }
}
