//! Real HTTP range requests against a loopback server serving a generated PMTiles fixture.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use fs_maps_core::testutil::{RangeServer, write_fixture};
use fs_maps_core::{
    BBox, CacheConfig, CachedSource, FallbackSource, PmtilesFileSource, PmtilesHttpSource,
    RegionDownload, RegionSpec, TileCache, TileSource,
};

const UA: &str =
    "FreeSynergy-Maps-test/0.1 (+https://freesynergy.example; contact: test@example.org)";

#[tokio::test]
async fn http_source_range_requests_cache_and_region() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = dir.path().join("fixture.pmtiles");
    write_fixture(&fixture, 6).await.unwrap();
    let total_size = std::fs::metadata(&fixture).unwrap().len();
    let server = RangeServer::start(&fixture).await.unwrap();

    let http = PmtilesHttpSource::open(&server.url, UA).await.unwrap();
    let after_open = server.requests.load(Ordering::SeqCst);
    assert!(
        after_open <= 2,
        "open = header+root dir only, got {after_open}"
    );
    let info = http.info().unwrap();
    assert_eq!((info.min_zoom, info.max_zoom), (0, 6));

    // pieces on demand: one tile = one (small) range request, never the whole file
    let t = http.get_tile(6, 33, 21).await.unwrap().unwrap();
    assert!(t.starts_with(b"MVT-FIXTURE 6/33/21 "));
    assert!(http.get_tile(7, 0, 0).await.unwrap().is_none());

    // User-Agent is ours on every request
    assert!(server.user_agents.lock().unwrap().iter().all(|u| u == UA));

    // through the cache: second read costs no request
    let cache =
        Arc::new(TileCache::open(CacheConfig::new(dir.path().join("cache"), 64 * 1024)).unwrap());
    let cached = CachedSource::new(http, cache.clone());
    cached.get_tile(5, 3, 4).await.unwrap().unwrap();
    let n = server.requests.load(Ordering::SeqCst);
    cached.get_tile(5, 3, 4).await.unwrap().unwrap();
    assert_eq!(server.requests.load(Ordering::SeqCst), n);
    let s = cache.stats();
    assert_eq!((s.hits, s.entries), (1, 1));

    // region download over HTTP (a small box, z0..4), then read it offline
    let spec = RegionSpec {
        name: "mini".into(),
        bbox: BBox::new(-10.0, 35.0, 30.0, 60.0).unwrap(),
        min_zoom: 0,
        max_zoom: 4,
        attribution: Some("© OpenStreetMap contributors".into()),
    };
    let rd = RegionDownload::new(
        Arc::new(PmtilesHttpSource::open(&server.url, UA).await.unwrap()),
        spec,
    )
    .unwrap();
    let est = rd.estimate(5).await.unwrap();
    assert!(est.tiles > 0 && est.avg_tile_bytes > 200);
    let out = dir.path().join("mini.pmtiles");
    let rep = rd.download(&out, &mut |_| true).await.unwrap();
    assert_eq!(rep.tiles_written, est.tiles);
    assert!(rep.file_bytes < total_size);

    // offline-first chain: region package, then the (now unreachable) network
    let region = PmtilesFileSource::open(&out).await.unwrap();
    drop(server);
    let chain = FallbackSource::new(vec![Arc::new(region)]);
    assert!(chain.get_tile(4, 8, 5).await.unwrap().is_some());
    assert!(chain.get_tile(6, 33, 21).await.unwrap().is_none());
}
