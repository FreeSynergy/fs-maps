//! What the cache does while its source is away, or no longer has a tile.

use std::sync::Arc;
use std::sync::atomic::Ordering;
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

/// A source that is away while `0` is true, and has nothing left while `1` is.
struct Away(std::sync::atomic::AtomicBool, std::sync::atomic::AtomicBool);

#[async_trait]
impl TileSource for Away {
    fn id(&self) -> String {
        "away".into()
    }
    async fn get_tile(&self, _: u8, _: u32, _: u32) -> Result<Option<Bytes>> {
        if self.0.load(Ordering::SeqCst) {
            return Err(fs_maps_core::Error::SourceUnavailable("away".into()));
        }
        let gone = self.1.load(Ordering::SeqCst);
        Ok((!gone).then(|| Bytes::from_static(b"fresh")))
    }
}

#[tokio::test]
async fn an_expired_tile_stays_until_a_fetch_replaced_it() {
    use std::sync::atomic::AtomicBool;
    let (_d, c) = cache(1000, Some(Duration::from_secs(100)));
    let long_ago = 1000;
    c.put_at("away", 2, 1, 1, b"old", long_ago).unwrap();
    let away = Away(AtomicBool::new(true), AtomicBool::new(false));
    let cs = CachedSource::new(away, Arc::new(c));

    // The source is away: better an old tile than none, and it is still there afterwards.
    for _ in 0..2 {
        assert_eq!(cs.get_tile(2, 1, 1).await.unwrap().unwrap(), &b"old"[..]);
    }
    // Nothing old to fall back on: the failure shows.
    assert!(cs.get_tile(2, 0, 0).await.is_err());

    // Back: the fetch replaces it.
    cs.inner().0.store(false, Ordering::SeqCst);
    assert_eq!(cs.get_tile(2, 1, 1).await.unwrap().unwrap(), &b"fresh"[..]);
    assert_eq!(cs.cache().stats().hits, 0);
    assert_eq!(cs.get_tile(2, 1, 1).await.unwrap().unwrap(), &b"fresh"[..]);
    assert_eq!(cs.cache().stats().hits, 1);
}

#[tokio::test]
async fn a_tile_the_source_no_longer_has_is_forgotten() {
    use std::sync::atomic::AtomicBool;
    let (_d, c) = cache(1000, Some(Duration::from_secs(100)));
    c.put_at("away", 2, 1, 1, b"old", 1000).unwrap();
    let cs = CachedSource::new(
        Away(AtomicBool::new(false), AtomicBool::new(true)),
        Arc::new(c),
    );
    assert_eq!(cs.get_tile(2, 1, 1).await.unwrap(), None);
    assert!(cs.cache().get_stale("away", 2, 1, 1).is_none());
}

#[tokio::test]
async fn a_tile_too_large_is_not_written_to_disk() {
    let mut m = MemorySource::new("big");
    m.insert(1, 0, 0, vec![0u8; fs_maps_core::MAX_TILE_BYTES + 1]);
    let (_d, c) = cache(1 << 30, None);
    let cs = CachedSource::new(m, Arc::new(c));
    assert!(cs.get_tile(1, 0, 0).await.unwrap().is_some());
    assert_eq!(cs.cache().disk_bytes(), 0);
    assert_eq!(cs.cache().stats().entries, 0);
}

/// A source whose archive changes while a read is on its way: the read still brings the old bytes.
struct Changes(std::sync::Mutex<&'static str>);

#[async_trait]
impl TileSource for Changes {
    fn id(&self) -> String {
        "changes".into()
    }
    fn version(&self) -> Option<String> {
        Some((*self.0.lock().unwrap()).to_string())
    }
    async fn get_tile(&self, _: u8, _: u32, _: u32) -> Result<Option<Bytes>> {
        let mut version = self.0.lock().unwrap();
        let bytes = Bytes::from_static(if *version == "v1" {
            b"v1 bytes"
        } else {
            b"v2 bytes"
        });
        *version = "v2";
        Ok(Some(bytes))
    }
}

#[tokio::test]
async fn a_read_under_way_when_the_archive_changes_is_not_kept_for_the_new_version() {
    let (_d, c) = cache(1000, None);
    let cs = CachedSource::new(Changes(std::sync::Mutex::new("v1")), Arc::new(c));
    assert_eq!(
        cs.get_tile(1, 0, 0).await.unwrap().unwrap(),
        &b"v1 bytes"[..]
    );
    assert_eq!(cs.cache().stats().entries, 0);
    assert_eq!(
        cs.get_tile(1, 0, 0).await.unwrap().unwrap(),
        &b"v2 bytes"[..]
    );
    assert_eq!(cs.cache().stats().entries, 1);
    assert_eq!(
        cs.get_tile(1, 0, 0).await.unwrap().unwrap(),
        &b"v2 bytes"[..]
    );
    assert_eq!(cs.cache().stats().hits, 1);
}
