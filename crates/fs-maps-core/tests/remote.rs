//! A remote archive on the loopback: replaced, lying, silent.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use fs_maps_core::testutil::{RangeServer, write_fixture, write_labelled};
use fs_maps_core::{
    CacheConfig, CachedSource, Error, FallbackSource, LazyHttpSource, PmtilesHttpSource, TileCache,
    TileSource,
};

const UA: &str = "FreeSynergy-Maps-test/0.2 (+https://freesynergy.example)";

fn labelled(label: &str, z: u8, x: u32, y: u32) -> Vec<u8> {
    format!("{label} {z}/{x}/{y} {}", "~".repeat(200)).into_bytes()
}

async fn served(dir: &std::path::Path, label: &str) -> RangeServer {
    let file = dir.join(format!("{label}.pmtiles"));
    write_labelled(&file, 2, label).await.unwrap();
    RangeServer::start(&file).await.unwrap()
}

#[tokio::test]
async fn a_tile_that_cannot_exist_reaches_no_server() {
    let dir = tempfile::tempdir().unwrap();
    let server = served(dir.path(), "T").await;
    let lazy = || LazyHttpSource::new(&server.url, UA).unwrap();
    let cache = Arc::new(TileCache::open(CacheConfig::new(dir.path().join("c"), 1 << 20)).unwrap());
    let cached = CachedSource::new(lazy(), cache);
    let chain = FallbackSource::new(vec![Arc::new(lazy())]);
    let sources: [&dyn TileSource; 3] = [&lazy(), &cached, &chain];
    for source in sources {
        for (z, x, y) in [(23, 0, 0), (1, 2, 0), (255, u32::MAX, 0)] {
            let e = source.get_tile(z, x, y).await.unwrap_err();
            assert!(matches!(e, Error::InvalidTile { .. }), "{e:?}");
        }
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_server_sending_more_than_asked_is_cut_off() {
    let dir = tempfile::tempdir().unwrap();
    let server = served(dir.path(), "P").await;
    server.pad_answers(1024 * 1024);
    let e = PmtilesHttpSource::open(&server.url, UA)
        .await
        .err()
        .unwrap();
    assert!(matches!(e, Error::Pmtiles(_)), "{e:?}");
}

#[tokio::test]
async fn an_archive_of_pictures_is_not_served() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("png.pmtiles");
    write_fixture(&file, 1).await.unwrap();
    let mut bytes = std::fs::read(&file).unwrap();
    bytes[99] = 2;
    std::fs::write(&file, bytes).unwrap();
    let server = RangeServer::start(&file).await.unwrap();
    let e = LazyHttpSource::new(&server.url, UA)
        .unwrap()
        .get_tile(0, 0, 0)
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Archive(_)), "{e:?}");
}

#[tokio::test]
async fn a_silent_host_makes_only_the_first_wait() {
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/map.pmtiles", silent.local_addr().unwrap());
    let source = LazyHttpSource::new(&url, UA)
        .unwrap()
        .with_timing(Duration::from_millis(200), Duration::from_secs(60));
    let began = Instant::now();
    let (a, b, c) = tokio::join!(
        source.get_tile(0, 0, 0),
        source.get_tile(1, 0, 0),
        source.get_tile(1, 1, 0)
    );
    assert!(a.is_err() && b.is_err() && c.is_err());
    assert!(
        began.elapsed() < Duration::from_millis(600),
        "{:?}",
        began.elapsed()
    );
    let began = Instant::now();
    let e = source.get_tile(0, 0, 0).await.unwrap_err();
    assert!(matches!(e, Error::SourceUnavailable(_)), "{e:?}");
    assert!(began.elapsed() < Duration::from_millis(50));
}

#[tokio::test]
async fn a_failed_open_is_remembered_and_then_tried_again() {
    let dir = tempfile::tempdir().unwrap();
    let server = served(dir.path(), "R").await;
    server.pad_answers(1);
    let source = LazyHttpSource::new(&server.url, UA)
        .unwrap()
        .with_timing(Duration::from_secs(5), Duration::from_millis(100));
    assert!(source.get_tile(0, 0, 0).await.is_err());
    let asked = server.requests.load(Ordering::SeqCst);
    server.pad_answers(0);
    assert!(source.get_tile(0, 0, 0).await.is_err());
    assert_eq!(
        server.requests.load(Ordering::SeqCst),
        asked,
        "asked within the backoff"
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        source.get_tile(0, 0, 0).await.unwrap().unwrap(),
        labelled("R", 0, 0, 0)
    );
}

/// A source that takes any number, as a foreign one may.
struct Trusting(AtomicUsize);

#[async_trait::async_trait]
impl TileSource for Trusting {
    fn id(&self) -> String {
        "trusting".into()
    }
    async fn get_tile(&self, _: u8, _: u32, _: u32) -> fs_maps_core::Result<Option<bytes::Bytes>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(None)
    }
}

#[tokio::test]
async fn a_chain_asks_nobody_about_a_tile_that_cannot_exist() {
    let asked = Arc::new(Trusting(AtomicUsize::new(0)));
    let chain = FallbackSource::new(vec![asked.clone()]);
    let e = chain.get_tile(1, 2, 0).await.unwrap_err();
    assert!(matches!(e, Error::InvalidTile { .. }), "{e:?}");
    assert_eq!(asked.0.load(Ordering::SeqCst), 0);
}
