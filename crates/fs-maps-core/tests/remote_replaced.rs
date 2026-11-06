//! A remote archive replaced while it is in use, on the loopback.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use fs_maps_core::testutil::{RangeServer, archive, directory, gzip, write_labelled};
use fs_maps_core::{CacheConfig, CachedSource, LazyHttpSource, TileCache, TileSource};

const UA: &str = "FreeSynergy-Maps-test/0.2 (+https://freesynergy.example)";
const NONE: u8 = 1;
const GZIP: u8 = 2;
const MVT: u8 = 1;

fn labelled(label: &str, z: u8, x: u32, y: u32) -> Vec<u8> {
    format!("{label} {z}/{x}/{y} {}", "~".repeat(200)).into_bytes()
}

async fn served(dir: &std::path::Path, label: &str) -> RangeServer {
    let file = dir.join(format!("{label}.pmtiles"));
    write_labelled(&file, 2, label).await.unwrap();
    RangeServer::start(&file).await.unwrap()
}

#[tokio::test]
async fn a_replaced_archive_is_opened_again_and_versions_do_not_mix() {
    let dir = tempfile::tempdir().unwrap();
    let server = served(dir.path(), "OLD").await;
    let cache = Arc::new(TileCache::open(CacheConfig::new(dir.path().join("c"), 1 << 20)).unwrap());
    let source = CachedSource::new(LazyHttpSource::new(&server.url, UA).unwrap(), cache);
    let tile = source.get_tile(1, 0, 0).await.unwrap().unwrap();
    assert_eq!(tile, labelled("OLD", 1, 0, 0));

    let new = dir.path().join("new.pmtiles");
    write_labelled(&new, 2, "NEW").await.unwrap();
    server.replace_archive(&new, "new").unwrap();

    // Still cached, as nothing has told it yet. The first uncached tile notices.
    let tile = source.get_tile(1, 0, 0).await.unwrap().unwrap();
    assert_eq!(tile, labelled("OLD", 1, 0, 0));
    let tile = source.get_tile(2, 1, 1).await.unwrap().unwrap();
    assert_eq!(tile, labelled("NEW", 2, 1, 1));
    // Now the old tile is not an answer any more.
    let tile = source.get_tile(1, 0, 0).await.unwrap().unwrap();
    assert_eq!(tile, labelled("NEW", 1, 0, 0));
    assert_eq!(source.version().as_deref(), Some("\"new\""));
}

#[tokio::test]
async fn a_replaced_archive_of_another_layout_is_opened_again() {
    let dir = tempfile::tempdir().unwrap();
    let server = served(dir.path(), "OLD").await;
    let source = LazyHttpSource::new(&server.url, UA).unwrap();
    source.get_tile(0, 0, 0).await.unwrap().unwrap();

    // Larger, with other bytes at every offset: they must not reach the decompression.
    let larger = dir.path().join("larger.pmtiles");
    write_labelled(&larger, 4, "LARGER").await.unwrap();
    server.replace_archive(&larger, "larger").unwrap();
    let before = server.requests.load(Ordering::SeqCst);
    let tile = source.get_tile(2, 1, 1).await.unwrap().unwrap();
    assert_eq!(tile, labelled("LARGER", 2, 1, 1));
    let used = server.requests.load(Ordering::SeqCst) - before;
    assert!(
        used <= 3,
        "{used} requests: one stale read, one open, one tile"
    );

    // Smaller, and the tile lies past its end: 416, and there the archive has no such tile.
    let tiny = dir.path().join("tiny.pmtiles");
    write_labelled(&tiny, 0, "TINY").await.unwrap();
    server.replace_archive(&tiny, "tiny").unwrap();
    assert_eq!(source.get_tile(4, 15, 15).await.unwrap(), None);
    let tile = source.get_tile(0, 0, 0).await.unwrap().unwrap();
    assert_eq!(tile, labelled("TINY", 0, 0, 0));
    assert_eq!(source.version().as_deref(), Some("\"tiny\""));
}

#[tokio::test]
async fn a_leaf_directory_of_another_version_is_not_taken_for_a_broken_one() {
    let dir = tempfile::tempdir().unwrap();
    let old_tile = b"old tile";
    let leaf = gzip(&directory(1, old_tile.len() as u64));
    let root = gzip(&directory(0, leaf.len() as u64));
    let old = dir.path().join("old.pmtiles");
    let bytes = archive([GZIP, NONE, MVT], &root, &leaf, old_tile, 8);
    std::fs::write(&old, bytes).unwrap();
    let server = RangeServer::start(&old).await.unwrap();
    let source = LazyHttpSource::new(&server.url, UA).unwrap();
    source.refresh().await.unwrap();

    // Where the leaf was there are now plain bytes, no gzip: only the version can say why.
    let new_tile = vec![b'n'; 1024];
    let root = gzip(&directory(1, 1024));
    let new = dir.path().join("new.pmtiles");
    let bytes = archive([GZIP, NONE, MVT], &root, &[], &new_tile, 1024);
    std::fs::write(&new, bytes).unwrap();
    server.replace_archive(&new, "new").unwrap();
    assert_eq!(source.get_tile(0, 0, 0).await.unwrap().unwrap(), new_tile);
}
