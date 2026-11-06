//! `TileCache` behaviour: stats, LRU eviction, size limit, TTL, restart, clear, and the
//! `CachedSource` decorator.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use fs_maps_core::{CacheConfig, CachedSource, MemorySource, Result, TileCache, TileSource};

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
    assert!(
        c.get("s", 5, 0, 0).is_some()
            && c.get("s", 5, 2, 0).is_some()
            && c.get("s", 5, 3, 0).is_some()
    );
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
    assert_eq!(c.stats().entries, 2);
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

struct Counting(MemorySource, AtomicU32);

#[async_trait]
impl TileSource for Counting {
    fn id(&self) -> String {
        self.0.id()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        self.1.fetch_add(1, Ordering::SeqCst);
        self.0.get_tile(z, x, y).await
    }
}

#[tokio::test]
async fn cached_source_hits_cache_second_time() {
    let mut m = MemorySource::new("mem");
    m.insert(2, 1, 1, &b"tile"[..]);
    let (_d, c) = cache(1000, None);
    let cs = CachedSource::new(Counting(m, AtomicU32::new(0)), Arc::new(c));
    assert_eq!(cs.get_tile(2, 1, 1).await.unwrap().unwrap(), &b"tile"[..]);
    assert_eq!(cs.get_tile(2, 1, 1).await.unwrap().unwrap(), &b"tile"[..]);
    assert_eq!(cs.inner().1.load(Ordering::SeqCst), 1);
    assert!(cs.get_tile(2, 0, 0).await.unwrap().is_none());
    assert_eq!(cs.cache().stats().entries, 1, "absent tiles are not cached");
}
