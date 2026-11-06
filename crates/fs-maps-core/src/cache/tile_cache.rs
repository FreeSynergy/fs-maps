//! The tile cache proper: size-limited, LRU, TTL, atomic writes.
//!
//! Pattern: Repository — get/put/clear over tiles keyed by `(source, z, x, y)`; the LRU index
//! (`index.rs`) and the disk layout (`disk.rs`) are collaborators.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;

use super::config::CacheConfig;
use super::disk::{HEADER, make_dirs, now_secs, path_of, scan, source_hash, touch, write_entry};
use super::index::{Entry, Inner};
use super::stats::CacheStats;
use crate::error::{Error, Result};

/// Size-limited on-disk LRU cache of raw MVT tiles.
pub struct TileCache {
    pub(super) cfg: Mutex<CacheConfig>,
    pub(super) inner: Mutex<Inner>,
    tmp_counter: AtomicU64,
    pub(super) versions: Mutex<HashMap<u64, String>>,
}

impl TileCache {
    /// Open (and scan) a cache directory; creates it if needed. If the directory holds more than
    /// `max_bytes` (limit lowered), the oldest entries are evicted immediately.
    pub fn open(cfg: CacheConfig) -> Result<Self> {
        make_dirs(&cfg.dir)?;
        let mut found = scan(&cfg.dir)?;
        found.sort_by_key(|f| f.0);
        let mut inner = Inner::new();
        for (_, k, size, stored_at) in found {
            inner.tick += 1;
            let tick = inner.tick;
            inner.insert(
                k,
                Entry {
                    size,
                    tick,
                    stored_at,
                },
            );
        }
        let cache = Self {
            cfg: Mutex::new(cfg),
            inner: Mutex::new(inner),
            tmp_counter: AtomicU64::new(0),
            versions: Mutex::new(HashMap::new()),
        };
        {
            let mut g = cache.inner.lock().expect("cache lock");
            cache.evict(&mut g);
        }
        Ok(cache)
    }

    pub(super) fn dir(&self) -> PathBuf {
        self.cfg.lock().expect("cfg lock").dir.clone()
    }

    /// Snapshot of counters and sizes.
    pub fn stats(&self) -> CacheStats {
        let g = self.inner.lock().expect("cache lock");
        let max = self.cfg.lock().expect("cfg lock").max_bytes;
        CacheStats {
            entries: g.map.len() as u64,
            bytes: g.total,
            max_bytes: max,
            ..g.stats
        }
    }

    /// Change the limit; evicts immediately if the cache is now too large.
    pub fn set_max_bytes(&self, max: u64) {
        self.cfg.lock().expect("cfg lock").max_bytes = max;
        let mut g = self.inner.lock().expect("cache lock");
        self.evict(&mut g);
    }

    /// Look a tile up. Hits refresh the LRU position; an expired tile is a miss, and kept.
    pub fn get(&self, source_id: &str, z: u8, x: u32, y: u32) -> Option<Bytes> {
        self.get_at(source_id, z, x, y, now_secs())
    }

    /// [`TileCache::get`] with an explicit clock (tests).
    #[doc(hidden)]
    pub fn get_at(&self, source_id: &str, z: u8, x: u32, y: u32, now: u64) -> Option<Bytes> {
        let key = (source_hash(source_id), z, x, y);
        let (dir, ttl) = {
            let c = self.cfg.lock().expect("cfg lock");
            (c.dir.clone(), c.ttl)
        };
        let path = path_of(&dir, key);
        let mut g = self.inner.lock().expect("cache lock");
        let Some(e) = g.map.get(&key) else {
            g.stats.misses += 1;
            return None;
        };
        if ttl.is_some_and(|ttl| now.saturating_sub(e.stored_at) > ttl.as_secs()) {
            // It stays until a fetch replaced it: the source may be away (`get_stale`).
            g.stats.expired += 1;
            g.stats.misses += 1;
            return None;
        }
        match fs::read(&path) {
            Ok(mut data) if data.len() >= HEADER => {
                g.touch(key);
                g.stats.hits += 1;
                drop(g);
                touch(&path);
                Some(Bytes::from(data.split_off(HEADER)))
            }
            _ => {
                // file vanished or is damaged: forget it
                g.remove(&key);
                let _ = fs::remove_file(&path);
                g.stats.misses += 1;
                None
            }
        }
    }

    /// Store a tile; evicts least-recently-used entries until the limit holds. A tile that alone
    /// exceeds the limit is not stored (returns `false`).
    pub fn put(&self, source_id: &str, z: u8, x: u32, y: u32, data: &[u8]) -> Result<bool> {
        self.put_at(source_id, z, x, y, data, now_secs())
    }

    /// [`TileCache::put`] with an explicit clock (tests).
    #[doc(hidden)]
    pub fn put_at(
        &self,
        source_id: &str,
        z: u8,
        x: u32,
        y: u32,
        data: &[u8],
        now: u64,
    ) -> Result<bool> {
        let key = (source_hash(source_id), z, x, y);
        let size = (HEADER + data.len()) as u64;
        let (dir, max) = {
            let c = self.cfg.lock().expect("cfg lock");
            (c.dir.clone(), c.max_bytes)
        };
        let path = path_of(&dir, key);
        let mut g = self.inner.lock().expect("cache lock");
        if size > max {
            if g.remove(&key).is_some() {
                let _ = fs::remove_file(&path);
            }
            return Ok(false);
        }
        g.remove(&key);
        let parent = path.parent().expect("has parent");
        make_dirs(parent)?;
        let tmp = parent.join(format!(
            ".tmp-{}",
            self.tmp_counter.fetch_add(1, Ordering::Relaxed)
        ));
        if let Err(e) = write_entry(&tmp, &path, now, data) {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file(&path);
            return Err(Error::io(&path, e));
        }
        g.tick += 1;
        let tick = g.tick;
        g.insert(
            key,
            Entry {
                size,
                tick,
                stored_at: now,
            },
        );
        self.evict(&mut g);
        Ok(true)
    }
}
