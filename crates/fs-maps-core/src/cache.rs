//! On-disk tile cache with a hard size limit and LRU eviction.
//!
//! Layout: `<dir>/<source-hash>/<z>/<x>/<y>.mvt`; each file is an 8-byte little-endian
//! "stored at" Unix timestamp followed by the raw MVT bytes. Recency is kept in memory and
//! persisted through the file modification time (touched on every hit), so the LRU order survives
//! restarts.
//!
//! Cache rules (RESEARCH.md): PMTiles tiles are immutable per archive version, so there are no
//! per-tile HTTP caching headers to honour; we apply the OSMF minimum of **7 days** as default
//! `ttl` (an expired entry is refetched). Nothing here ever sends `no-cache` headers.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;

use crate::error::{Error, Result};
use crate::source::{SourceInfo, TileSource};

const HEADER: usize = 8;
const FILE_EXT: &str = "mvt";

#[derive(Clone, Debug)]
pub struct CacheConfig {
    pub dir: PathBuf,
    /// Hard limit for the sum of all file sizes (including the 8-byte headers).
    pub max_bytes: u64,
    /// Entries older than this are treated as missing. `None` = never expire.
    pub ttl: Option<Duration>,
}

impl CacheConfig {
    pub fn new(dir: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Self {
            dir: dir.into(),
            max_bytes,
            ttl: Some(Duration::from_secs(7 * 24 * 3600)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: u64,
    pub bytes: u64,
    pub max_bytes: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub expired: u64,
}

type Key = (u64, u8, u32, u32);

struct Entry {
    size: u64,
    tick: u64,
    stored_at: u64,
}

struct Inner {
    map: HashMap<Key, Entry>,
    order: BTreeMap<u64, Key>,
    total: u64,
    tick: u64,
    stats: CacheStats,
}

pub struct TileCache {
    cfg: Mutex<CacheConfig>,
    inner: Mutex<Inner>,
    tmp_counter: AtomicU64,
}

/// FNV-1a 64: stable across runs and platforms (directory names must not change).
pub fn source_hash(source_id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source_id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn path_of(dir: &Path, k: Key) -> PathBuf {
    dir.join(format!("{:016x}", k.0))
        .join(k.1.to_string())
        .join(k.2.to_string())
        .join(format!("{}.{FILE_EXT}", k.3))
}

impl Inner {
    fn insert(&mut self, k: Key, e: Entry) {
        self.total += e.size;
        self.order.insert(e.tick, k);
        self.map.insert(k, e);
    }
    fn remove(&mut self, k: &Key) -> Option<Entry> {
        let e = self.map.remove(k)?;
        self.order.remove(&e.tick);
        self.total -= e.size;
        Some(e)
    }
}

impl TileCache {
    /// Open (and scan) a cache directory; creates it if needed. If the directory holds more than
    /// `max_bytes` (limit lowered), the oldest entries are evicted immediately.
    pub fn open(cfg: CacheConfig) -> Result<Self> {
        fs::create_dir_all(&cfg.dir).map_err(|e| Error::io(&cfg.dir, e))?;
        let mut found: Vec<(SystemTime, Key, u64, u64)> = Vec::new();
        for src in read_dir(&cfg.dir)? {
            let Some(h) = src.file_name().to_str().and_then(|n| u64::from_str_radix(n, 16).ok()) else {
                continue;
            };
            if !src.path().is_dir() {
                continue;
            }
            for z in read_dir(&src.path())? {
                let Some(zv) = z.file_name().to_str().and_then(|n| n.parse::<u8>().ok()) else { continue };
                for x in read_dir(&z.path())? {
                    let Some(xv) = x.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
                    for f in read_dir(&x.path())? {
                        let p = f.path();
                        let name = f.file_name();
                        let name = name.to_string_lossy();
                        if name.starts_with(".tmp-") {
                            let _ = fs::remove_file(&p);
                            continue;
                        }
                        let Some(yv) = name
                            .strip_suffix(&format!(".{FILE_EXT}"))
                            .and_then(|n| n.parse::<u32>().ok())
                        else {
                            continue;
                        };
                        let Ok(md) = f.metadata() else { continue };
                        if md.len() < HEADER as u64 {
                            let _ = fs::remove_file(&p);
                            continue;
                        }
                        let stored = read_stored_at(&p).unwrap_or(0);
                        found.push((md.modified().unwrap_or(UNIX_EPOCH), (h, zv, xv, yv), md.len(), stored));
                    }
                }
            }
        }
        found.sort_by_key(|f| f.0);
        let mut inner = Inner {
            map: HashMap::new(),
            order: BTreeMap::new(),
            total: 0,
            tick: 0,
            stats: CacheStats::default(),
        };
        for (_, k, size, stored_at) in found {
            inner.tick += 1;
            let tick = inner.tick;
            inner.insert(k, Entry { size, tick, stored_at });
        }
        let cache = Self {
            cfg: Mutex::new(cfg),
            inner: Mutex::new(inner),
            tmp_counter: AtomicU64::new(0),
        };
        {
            let mut g = cache.inner.lock().expect("cache lock");
            cache.evict(&mut g);
        }
        Ok(cache)
    }

    fn dir(&self) -> PathBuf {
        self.cfg.lock().expect("cfg lock").dir.clone()
    }

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

    pub fn set_max_bytes(&self, max: u64) {
        self.cfg.lock().expect("cfg lock").max_bytes = max;
        let mut g = self.inner.lock().expect("cache lock");
        self.evict(&mut g);
    }

    /// Look a tile up. Hits refresh the LRU position.
    pub fn get(&self, source_id: &str, z: u8, x: u32, y: u32) -> Option<Bytes> {
        self.get_at(source_id, z, x, y, now_secs())
    }

    pub(crate) fn get_at(&self, source_id: &str, z: u8, x: u32, y: u32, now: u64) -> Option<Bytes> {
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
            g.remove(&key);
            let _ = fs::remove_file(&path);
            g.stats.expired += 1;
            g.stats.misses += 1;
            return None;
        }
        match fs::read(&path) {
            Ok(mut data) if data.len() >= HEADER => {
                let old = g.map[&key].tick;
                g.order.remove(&old);
                g.tick += 1;
                let t = g.tick;
                g.order.insert(t, key);
                g.map.get_mut(&key).expect("present").tick = t;
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

    pub(crate) fn put_at(&self, source_id: &str, z: u8, x: u32, y: u32, data: &[u8], now: u64) -> Result<bool> {
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
        fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        let tmp = parent.join(format!(".tmp-{}", self.tmp_counter.fetch_add(1, Ordering::Relaxed)));
        let write = || -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&now.to_le_bytes())?;
            f.write_all(data)?;
            f.sync_data().ok();
            fs::rename(&tmp, &path)
        };
        if let Err(e) = write() {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file(&path);
            return Err(Error::io(&path, e));
        }
        g.tick += 1;
        let tick = g.tick;
        g.insert(key, Entry { size, tick, stored_at: now });
        self.evict(&mut g);
        Ok(true)
    }

    fn evict(&self, g: &mut Inner) {
        let (dir, max) = {
            let c = self.cfg.lock().expect("cfg lock");
            (c.dir.clone(), c.max_bytes)
        };
        while g.total > max {
            let Some((&t, &k)) = g.order.iter().next() else { break };
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

    /// Keys in LRU order, oldest first (tests / diagnostics).
    #[doc(hidden)]
    pub fn keys_oldest_first(&self) -> Vec<(u64, u8, u32, u32)> {
        self.inner.lock().expect("cache lock").order.values().copied().collect()
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

fn read_dir(p: &Path) -> Result<Vec<fs::DirEntry>> {
    Ok(fs::read_dir(p).map_err(|e| Error::io(p, e))?.flatten().collect())
}

fn read_stored_at(p: &Path) -> Option<u64> {
    use std::io::Read;
    let mut b = [0u8; HEADER];
    fs::File::open(p).ok()?.read_exact(&mut b).ok()?;
    Some(u64::from_le_bytes(b))
}

fn touch(p: &Path) {
    if let Ok(f) = fs::OpenOptions::new().write(true).open(p) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// A source that reads through a [`TileCache`]: hit -> cache, miss -> inner source, then store.
pub struct CachedSource<S> {
    inner: S,
    cache: std::sync::Arc<TileCache>,
}

impl<S: TileSource> CachedSource<S> {
    pub fn new(inner: S, cache: std::sync::Arc<TileCache>) -> Self {
        Self { inner, cache }
    }
    pub fn cache(&self) -> &TileCache {
        &self.cache
    }
}

#[async_trait]
impl<S: TileSource> TileSource for CachedSource<S> {
    fn id(&self) -> String {
        self.inner.id()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        let id = self.inner.id();
        if let Some(t) = self.cache.get(&id, z, x, y) {
            return Ok(Some(t));
        }
        let t = self.inner.get_tile(z, x, y).await?;
        if let Some(t) = &t {
            // a failing cache write must not fail the read
            let _ = self.cache.put(&id, z, x, y, t);
        }
        Ok(t)
    }
    fn info(&self) -> Option<SourceInfo> {
        self.inner.info()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::VecDeque;

    fn cache(max: u64, ttl: Option<Duration>) -> (tempfile::TempDir, TileCache) {
        let d = tempfile::tempdir().unwrap();
        let mut cfg = CacheConfig::new(d.path(), max);
        cfg.ttl = ttl;
        let c = TileCache::open(cfg).unwrap();
        (d, c)
    }

    #[test]
    fn put_get_and_stats() {
        let (_d, c) = cache(1000, None);
        assert!(c.get("s", 1, 0, 0).is_none());
        assert!(c.put("s", 1, 0, 0, b"hello").unwrap());
        assert_eq!(c.get("s", 1, 0, 0).unwrap(), &b"hello"[..]);
        assert!(c.get("other", 1, 0, 0).is_none(), "keyed by source");
        let s = c.stats();
        assert_eq!((s.entries, s.hits, s.misses, s.bytes), (1, 1, 2, 13));
    }

    #[test]
    fn evicts_least_recently_used() {
        // each entry = 8 + 12 = 20 bytes; limit holds 3
        let (_d, c) = cache(60, None);
        for i in 0..3 {
            c.put("s", 5, i, 0, &[0u8; 12]).unwrap();
        }
        assert!(c.get("s", 5, 0, 0).is_some()); // refresh 0 -> 1 is oldest
        c.put("s", 5, 3, 0, &[0u8; 12]).unwrap();
        assert!(c.get("s", 5, 1, 0).is_none());
        assert!(c.get("s", 5, 0, 0).is_some() && c.get("s", 5, 2, 0).is_some() && c.get("s", 5, 3, 0).is_some());
        assert_eq!(c.stats().evictions, 1);
        assert_eq!(c.disk_bytes(), 60);
    }

    #[test]
    fn oversized_tile_not_stored() {
        let (_d, c) = cache(10, None);
        assert!(!c.put("s", 0, 0, 0, &[0u8; 100]).unwrap());
        assert_eq!(c.stats().bytes, 0);
    }

    #[test]
    fn ttl_expiry() {
        let (_d, c) = cache(1000, Some(Duration::from_secs(100)));
        c.put_at("s", 0, 0, 0, b"x", 1000).unwrap();
        assert!(c.get_at("s", 0, 0, 0, 1100).is_some());
        assert!(c.get_at("s", 0, 0, 0, 1101).is_none());
        assert_eq!(c.stats().expired, 1);
        assert_eq!(c.disk_bytes(), 0);
    }

    #[test]
    fn survives_restart_and_lowered_limit() {
        let d = tempfile::tempdir().unwrap();
        {
            let c = TileCache::open(CacheConfig::new(d.path(), 1000)).unwrap();
            for i in 0..5u32 {
                c.put("s", 5, i, 0, &[7u8; 12]).unwrap();
                std::thread::sleep(Duration::from_millis(15)); // distinct mtimes
            }
        }
        let c = TileCache::open(CacheConfig::new(d.path(), 45)).unwrap(); // room for 2
        let s = c.stats();
        assert_eq!(s.entries, 2);
        assert!(c.get("s", 5, 4, 0).is_some() && c.get("s", 5, 3, 0).is_some());
        assert!(c.get("s", 5, 0, 0).is_none());
    }

    #[test]
    fn clear_empties_disk() {
        let (_d, c) = cache(1000, None);
        c.put("s", 1, 1, 1, b"abc").unwrap();
        c.clear().unwrap();
        assert_eq!((c.stats().entries, c.disk_bytes()), (0, 0));
    }

    #[tokio::test]
    async fn cached_source_hits_cache_second_time() {
        use crate::source::MemorySource;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        struct Counting(MemorySource, AtomicU32);
        #[async_trait]
        impl TileSource for Counting {
            fn id(&self) -> String { self.0.id() }
            async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
                self.1.fetch_add(1, Ordering::SeqCst);
                self.0.get_tile(z, x, y).await
            }
        }
        let mut m = MemorySource::new("mem");
        m.insert(2, 1, 1, &b"tile"[..]);
        let (_d, c) = cache(1000, None);
        let src = Counting(m, AtomicU32::new(0));
        let cs = CachedSource::new(src, Arc::new(c));
        assert_eq!(cs.get_tile(2, 1, 1).await.unwrap().unwrap(), &b"tile"[..]);
        assert_eq!(cs.get_tile(2, 1, 1).await.unwrap().unwrap(), &b"tile"[..]);
        assert_eq!(cs.inner.1.load(Ordering::SeqCst), 1);
        assert!(cs.get_tile(2, 0, 0).await.unwrap().is_none());
        assert_eq!(cs.cache().stats().entries, 1, "absent tiles are not cached");
    }

    #[derive(Debug, Clone)]
    enum Op {
        Put(u8, usize),
        Get(u8),
    }

    fn ops() -> impl Strategy<Value = Vec<Op>> {
        prop::collection::vec(
            prop_oneof![
                (0u8..12, 0usize..60).prop_map(|(k, n)| Op::Put(k, n)),
                (0u8..12).prop_map(Op::Get),
            ],
            1..80,
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// The cache never exceeds its limit (index and disk), and behaves exactly like a
        /// reference LRU model.
        #[test]
        fn size_limit_and_lru_model(ops in ops(), max in 20u64..300) {
            let (_d, c) = cache(max, None);
            let mut model: VecDeque<(u8, u64)> = VecDeque::new(); // oldest first
            for op in ops {
                match op {
                    Op::Put(k, n) => {
                        let size = (HEADER + n) as u64;
                        let stored = c.put("s", 3, u32::from(k), 0, &vec![k; n]).unwrap();
                        model.retain(|e| e.0 != k);
                        if size <= max {
                            prop_assert!(stored);
                            model.push_back((k, size));
                            while model.iter().map(|e| e.1).sum::<u64>() > max {
                                model.pop_front();
                            }
                        } else {
                            prop_assert!(!stored);
                        }
                    }
                    Op::Get(k) => {
                        let got = c.get("s", 3, u32::from(k), 0);
                        let pos = model.iter().position(|e| e.0 == k);
                        prop_assert_eq!(got.is_some(), pos.is_some());
                        if let Some(p) = pos {
                            let e = model.remove(p).unwrap();
                            model.push_back(e);
                        }
                    }
                }
                let s = c.stats();
                prop_assert!(s.bytes <= max);
                prop_assert_eq!(s.bytes, model.iter().map(|e| e.1).sum::<u64>());
                prop_assert_eq!(c.disk_bytes(), s.bytes);
            }
            let order: Vec<u32> = c.keys_oldest_first().iter().map(|k| k.2).collect();
            let want: Vec<u32> = model.iter().map(|e| u32::from(e.0)).collect();
            prop_assert_eq!(order, want);
        }
    }
}
