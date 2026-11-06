//! The `TileSource` interface over a remote archive on the loopback.

mod common;

use std::sync::atomic::Ordering;

use common::{INVALID, is, problem, refused, with_tiles};
use fs_interfaces::Capabilities;
use fs_interfaces::places_capnp::tile_source;
use fs_maps_core::CacheConfig;
use fs_maps_core::testutil::{RangeServer, write_fixture};
use fs_maps_server::{TileConfig, open_tiles};

const SOURCE_FAILED: &str = "https://freesynergy.net/problems/places/tile-source-failed";
const UA: &str = "FreeSynergy-Maps-test/0.2 (+https://freesynergy.example)";

async fn tile(t: &tile_source::Client, z: u8, x: u32, y: u32) -> capnp::Result<Option<Vec<u8>>> {
    let mut req = t.tile_request();
    req.get().set_z(z);
    req.get().set_x(x);
    req.get().set_y(y);
    let answer = req.send().promise.await?;
    let r = answer.get()?;
    Ok(r.get_found().then(|| r.get_mvt().unwrap().to_vec()))
}

fn fixture_tile(z: u8, x: u32, y: u32) -> Vec<u8> {
    format!("MVT-FIXTURE {z}/{x}/{y} {}", "~".repeat(200)).into_bytes()
}

async fn id(t: &tile_source::Client) -> String {
    let answer = t.id_request().send().promise.await.unwrap();
    answer.get().unwrap().get_id().unwrap().to_string().unwrap()
}

async fn provider(t: &tile_source::Client) -> String {
    let answer = t.support_report_request().send().promise.await.unwrap();
    let report = answer.get().unwrap().get_report().unwrap();
    Capabilities::read(report).unwrap().provider().to_string()
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
    let requests = || server.requests.load(Ordering::SeqCst);
    assert_eq!(requests(), 0, "opening must not touch the network");
    with_tiles(source, |maps| async move {
        let t = maps.tiles;
        assert_eq!(
            tile(&t, 1, 0, 0).await.unwrap().unwrap(),
            fixture_tile(1, 0, 0)
        );
        assert_eq!(requests(), 0, "the local package answered");
        assert_eq!(
            tile(&t, 3, 1, 1).await.unwrap().unwrap(),
            fixture_tile(3, 1, 1)
        );
        let after_first = requests();
        assert!(after_first > 0);
        assert_eq!(
            tile(&t, 3, 1, 1).await.unwrap().unwrap(),
            fixture_tile(3, 1, 1)
        );
        assert_eq!(requests(), after_first, "the cache answered");
        assert_eq!(tile(&t, 4, 0, 0).await.unwrap(), None);

        let before = requests();
        for (z, x, y) in [(23, 0, 0), (1, 2, 0), (3, 8, 0), (255, u32::MAX, 0)] {
            assert_eq!(
                refused(tile(&t, z, x, y).await),
                is(INVALID, 422),
                "{z}/{x}/{y}"
            );
        }
        assert_eq!(requests(), before, "an invalid tile reached the server");
    })
    .await;
    let agents = server.user_agents.lock().unwrap();
    assert!(!agents.is_empty() && agents.iter().all(|u| u == UA));
}

#[tokio::test(flavor = "current_thread")]
async fn credentials_in_the_url_reach_no_consumer() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("remote.pmtiles");
    write_fixture(&file, 1).await.unwrap();
    let server = RangeServer::start(&file).await.unwrap();
    let url = format!(
        "{}?token=secret",
        server.url.replace("http://", "http://me:secret@")
    );
    let config = TileConfig {
        url: Some(url),
        user_agent: UA.into(),
        ..TileConfig::default()
    };
    let public = server.url.clone();
    with_tiles(open_tiles(&config).await.unwrap(), |maps| async move {
        assert_eq!(
            tile(&maps.tiles, 1, 0, 0).await.unwrap().unwrap(),
            fixture_tile(1, 0, 0)
        );
        assert_eq!(id(&maps.tiles).await, public);
        assert_eq!(provider(&maps.tiles).await, format!("tiles:{public}"));
    })
    .await;

    let dead = TileConfig {
        url: Some("http://me:secret@127.0.0.1:9/map.pmtiles?token=secret".into()),
        user_agent: UA.into(),
        ..TileConfig::default()
    };
    with_tiles(open_tiles(&dead).await.unwrap(), |maps| async move {
        for _ in 0..2 {
            let e = tile(&maps.tiles, 0, 0, 0).await.err().unwrap();
            let p = problem(&e);
            assert_eq!(p.status, 502);
            assert!(
                !p.detail.contains("secret") && !p.detail.contains("token"),
                "{}",
                p.detail
            );
        }
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
        assert_eq!(
            refused(tile(&maps.tiles, 0, 0, 0).await),
            is(SOURCE_FAILED, 502)
        );
    })
    .await;
}
