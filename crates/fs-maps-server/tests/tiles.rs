//! The `TileSource` interface reached through `Places.tiles`, over local PMTiles.

mod common;

use std::sync::Arc;

use common::{Consumer, TOO_LARGE, is, refused, with_server, with_tiles};
use fs_interfaces::places_capnp::tile_source;
use fs_interfaces::rpc::{MAX_PAGE_BYTES, Problem};
use fs_interfaces::{Capabilities, UNSUPPORTED_PROBLEM_TYPE};
use fs_maps_core::MemorySource;
use fs_maps_core::testutil::write_fixture;
use fs_maps_server::{TileConfig, open_tiles};

const INVALID: &str = common::INVALID;

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

/// Contract version, and the report.
async fn report(t: &tile_source::Client) -> (String, Capabilities) {
    let answer = t.support_report_request().send().promise.await.unwrap();
    let r = answer.get().unwrap().get_report().unwrap();
    let version = r.get_contract_version().unwrap().to_string().unwrap();
    (version, Capabilities::read(r).unwrap())
}

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
        assert_eq!(
            tile(&t, 3, 2, 5).await.unwrap().unwrap(),
            fixture_tile(3, 2, 5)
        );
        assert_eq!(
            tile(&t, 0, 0, 0).await.unwrap().unwrap(),
            fixture_tile(0, 0, 0)
        );
        assert_eq!(tile(&t, 4, 0, 0).await.unwrap(), None, "beyond the archive");

        let name = format!("file:{}", file.display());
        assert_eq!(id(&t).await, name);
        let (version, caps) = report(&t).await;
        assert_eq!(version, "places 0.1 (draft)");
        assert_eq!(caps.provider(), format!("tiles:{name}"));
        assert_eq!(caps.entries().len(), 1);
        assert!(caps.is_supported("tile.read"));

        for (z, x, y) in [
            (23, 0, 0),
            (255, 0, 0),
            (1, 2, 0),
            (1, 0, 2),
            (22, u32::MAX, 0),
            (0, 1, 0),
            (31, 1 << 31, 1 << 31),
        ] {
            assert_eq!(
                refused(tile(&t, z, x, y).await),
                is(INVALID, 422),
                "{z}/{x}/{y}"
            );
        }
        assert!(tile(&t, 0, 0, 0).await.is_ok(), "the connection survived");
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
        assert_eq!(p.status, 501);
        assert_eq!(p.detail, "tile.read: no tile source is configured");
        assert_eq!(id(&t).await, "none");
        let (version, caps) = report(&t).await;
        assert_eq!(version, "places 0.1 (draft)");
        assert_eq!(caps.provider(), "tiles:none");
        assert_eq!(caps.entries().len(), 1);
        assert!(!caps.is_supported("tile.read"));
        let why = caps.why_not("tile.read").unwrap();
        assert_eq!(why.reason(), "no tile source is configured");
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_tile_larger_than_a_message_is_refused_and_the_connection_lives() {
    let mut memory = MemorySource::new("memory");
    memory.insert(1, 0, 0, vec![7u8; MAX_PAGE_BYTES + 1]);
    memory.insert(1, 0, 1, vec![9u8; MAX_PAGE_BYTES]);
    with_tiles(Arc::new(memory), |maps| async move {
        let t = maps.tiles;
        assert_eq!(refused(tile(&t, 1, 0, 0).await), is(TOO_LARGE, 413));
        let largest = tile(&t, 1, 0, 1).await.unwrap().unwrap();
        assert_eq!(largest.len(), MAX_PAGE_BYTES);
        assert!(largest.iter().all(|b| *b == 9));
    })
    .await;
}

#[test]
fn a_tile_the_limit_allows_fits_an_answer() {
    assert!(fs_maps_core::MAX_TILE_BYTES <= MAX_PAGE_BYTES);
}
