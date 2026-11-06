//! The `TileSource` interface on the same socket, over local PMTiles and a loopback server.

mod common;

use std::sync::Arc;

use common::{Consumer, problem, with_server, with_tiles};
use fs_interfaces::places_capnp::tile_source;
use fs_interfaces::rpc::{MAX_PAGE_BYTES, Problem};
use fs_interfaces::{Capabilities, UNSUPPORTED_PROBLEM_TYPE};
use fs_maps_core::testutil::{RangeServer, write_fixture};
use fs_maps_core::{CacheConfig, MemorySource};
use fs_maps_server::{TileConfig, open_tiles};

async fn tile(t: &tile_source::Client, z: u8, x: u32, y: u32) -> capnp::Result<Option<Vec<u8>>> {
    let mut req = t.tile_request();
    req.get().set_z(z);
    req.get().set_x(x);
    req.get().set_y(y);
    let answer = req.send().promise.await?;
    let r = answer.get()?;
    Ok(r.get_found().then(|| r.get_mvt().unwrap().to_vec()))
}

async fn report(t: &tile_source::Client) -> Capabilities {
    let answer = t.support_report_request().send().promise.await.unwrap();
    Capabilities::read(answer.get().unwrap().get_report().unwrap()).unwrap()
}

async fn id(t: &tile_source::Client) -> String {
    let answer = t.id_request().send().promise.await.unwrap();
    answer.get().unwrap().get_id().unwrap().to_string().unwrap()
}

const UA: &str = "FreeSynergy-Maps-test/0.2 (+https://freesynergy.example)";

#[tokio::test(flavor = "current_thread")]
async fn a_local_package_answers_tile_by_tile() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("region.pmtiles");
    write_fixture(&file, 3).await.unwrap();
    let config = TileConfig {
        files: vec![file.clone()],
        ..TileConfig::default()
    };
    let source = open_tiles(&config).await.unwrap();
    with_tiles(source, |maps: Consumer| async move {
        let t = maps.tiles;
        let mvt = tile(&t, 3, 2, 5).await.unwrap().unwrap();
        assert!(mvt.starts_with(b"MVT-FIXTURE 3/2/5 "));
        assert_eq!(tile(&t, 4, 0, 0).await.unwrap(), None, "beyond the archive");
        assert!(id(&t).await.starts_with("file:"));
        assert!(report(&t).await.is_supported("tile.read"));

        for (z, x, y) in [(23, 0, 0), (1, 2, 0), (1, 0, 2)] {
            let e = tile(&t, z, x, y).await.err().unwrap();
            assert_eq!(problem(&e).status, 422);
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn without_a_source_tiles_are_not_possible() {
    with_server(|maps| async move {
        let t = maps.tiles;
        let e = tile(&t, 0, 0, 0).await.err().unwrap();
        let p = Problem::from_error(&e).unwrap();
        assert_eq!(p.problem_type, UNSUPPORTED_PROBLEM_TYPE);
        assert!(p.detail.starts_with("tile.read"), "{}", p.detail);
        assert_eq!(id(&t).await, "none");
        let caps = report(&t).await;
        assert!(!caps.is_supported("tile.read"));
        assert!(caps.why_not("tile.read").is_some());
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_tile_larger_than_a_message_is_refused_and_the_connection_lives() {
    let mut memory = MemorySource::new("memory");
    memory.insert(1, 0, 0, vec![7u8; MAX_PAGE_BYTES + 1]);
    memory.insert(1, 0, 1, vec![7u8; MAX_PAGE_BYTES]);
    with_tiles(Arc::new(memory), |maps| async move {
        let t = maps.tiles;
        let e = tile(&t, 1, 0, 0).await.err().unwrap();
        assert_eq!(problem(&e).status, 413);
        assert_eq!(
            tile(&t, 1, 0, 1).await.unwrap().unwrap().len(),
            MAX_PAGE_BYTES
        );
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_remote_archive_reads_through_the_cache_after_the_local_package() {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote.pmtiles");
    let local = dir.path().join("local.pmtiles");
    write_fixture(&remote, 3).await.unwrap();
    write_fixture(&local, 1).await.unwrap();
    let server = RangeServer::start(&remote).await.unwrap();
    let config = TileConfig {
        files: vec![local],
        url: Some(server.url.clone()),
        user_agent: UA.into(),
        cache: Some(CacheConfig::new(dir.path().join("cache"), 1 << 20)),
    };
    let source = open_tiles(&config).await.unwrap();
    assert_eq!(server.requests.load(std::sync::atomic::Ordering::SeqCst), 0);
    with_tiles(source, |maps| async move {
        let t = maps.tiles;
        assert!(tile(&t, 1, 0, 0).await.unwrap().is_some());
        let requests = || server.requests.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(requests(), 0, "the local package answered");
        assert!(tile(&t, 3, 1, 1).await.unwrap().is_some());
        let after_first = requests();
        assert!(after_first > 0);
        assert!(tile(&t, 3, 1, 1).await.unwrap().is_some());
        assert_eq!(requests(), after_first, "the cache answered");
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn an_unreachable_archive_is_a_typed_failure_not_a_start_failure() {
    let config = TileConfig {
        url: Some("http://127.0.0.1:9/map.pmtiles".into()),
        user_agent: UA.into(),
        ..TileConfig::default()
    };
    let source = open_tiles(&config).await.unwrap();
    with_tiles(source, |maps| async move {
        let e = tile(&maps.tiles, 0, 0, 0).await.err().unwrap();
        let p = problem(&e);
        assert_eq!(p.status, 502);
        assert_eq!(
            p.problem_type,
            "https://freesynergy.net/problems/places/tile-source-failed"
        );
    })
    .await;
}

#[tokio::test]
async fn a_forbidden_or_broken_configuration_does_not_start() {
    let forbidden = TileConfig {
        url: Some("https://tile.openstreetmap.org/x.pmtiles".into()),
        user_agent: UA.into(),
        ..TileConfig::default()
    };
    assert!(open_tiles(&forbidden).await.is_err());
    let missing = TileConfig {
        files: vec!["/nonexistent/region.pmtiles".into()],
        ..TileConfig::default()
    };
    assert!(open_tiles(&missing).await.is_err());
}
