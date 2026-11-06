//! Region download: estimate, limits, roundtrip through a local PMTiles file, cancel, clamping.

use std::sync::Arc;

use fs_maps_core::{
    BBox, Error, MemorySource, PmtilesFileSource, RegionDownload, RegionSpec, TileSource,
};

/// Source with a tile for every coordinate of zoom 0..=4 whose payload encodes the coordinate.
fn full_source() -> MemorySource {
    let mut m = MemorySource::new("mem:full");
    for z in 0..=4u8 {
        for x in 0..(1u32 << z) {
            for y in 0..(1u32 << z) {
                m.insert(
                    z,
                    x,
                    y,
                    format!("{z}/{x}/{y}-{}", "x".repeat(20)).into_bytes(),
                );
            }
        }
    }
    m
}

fn spec(bbox: BBox, lo: u8, hi: u8) -> RegionSpec {
    RegionSpec {
        name: "t".into(),
        bbox,
        min_zoom: lo,
        max_zoom: hi,
        attribution: Some("© OpenStreetMap contributors".into()),
    }
}

fn world() -> BBox {
    BBox::new(-180.0, -85.0, 180.0, 85.0).unwrap()
}

#[tokio::test]
async fn estimate_counts_without_requests() {
    let rd = RegionDownload::new(Arc::new(full_source()), spec(world(), 0, 3)).unwrap();
    let e = rd.estimate(0).await.unwrap();
    assert_eq!(e.tiles, 1 + 4 + 16 + 64);
    assert_eq!(e.sampled, 0);
    let e = rd.estimate(10).await.unwrap();
    assert_eq!(e.sampled, 10);
    assert!(e.avg_tile_bytes > 20 && e.est_bytes == e.avg_tile_bytes * e.tiles);
}

#[tokio::test]
async fn too_many_tiles_refused() {
    let rd = RegionDownload::new(Arc::new(full_source()), spec(world(), 0, 4))
        .unwrap()
        .with_max_tiles(50);
    assert!(matches!(
        rd.estimate(0).await,
        Err(Error::TooManyTiles { .. })
    ));
}

#[tokio::test]
async fn download_roundtrip_through_local_pmtiles() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("r.pmtiles");
    let b = BBox::new(0.0, 0.0, 90.0, 60.0).unwrap(); // NE quadrant-ish
    let rd = RegionDownload::new(Arc::new(full_source()), spec(b, 1, 3)).unwrap();
    let est = rd.estimate(0).await.unwrap();
    let mut calls = 0u64;
    let rep = rd
        .download(&out, &mut |p| {
            calls += 1;
            assert_eq!(p.total, est.tiles);
            true
        })
        .await
        .unwrap();
    assert_eq!(calls, est.tiles);
    assert_eq!((rep.tiles_written, rep.tiles_missing), (est.tiles, 0));
    assert!(!dir.path().join("r.pmtiles.part").exists());

    let local = PmtilesFileSource::open(&out).await.unwrap();
    let info = local.info().unwrap();
    assert_eq!((info.min_zoom, info.max_zoom), (1, 3));
    // every tile in the region is there with identical bytes; tiles outside are not
    let full = full_source();
    for z in 1..=3u8 {
        let ((x0, x1), (y0, y1)) = b.tile_range(z).unwrap();
        for x in x0..=x1 {
            for y in y0..=y1 {
                assert_eq!(
                    local.get_tile(z, x, y).await.unwrap(),
                    full.get_tile(z, x, y).await.unwrap(),
                    "{z}/{x}/{y}"
                );
            }
        }
    }
    assert!(local.get_tile(3, 0, 7).await.unwrap().is_none());
}

#[tokio::test]
async fn cancel_removes_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("c.pmtiles");
    let rd = RegionDownload::new(Arc::new(full_source()), spec(world(), 0, 3)).unwrap();
    let r = rd.download(&out, &mut |p| p.done < 5).await;
    assert!(matches!(r, Err(Error::Cancelled)));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn zoom_clamped_to_source_and_missing_counted() {
    let mut m = MemorySource::new("sparse");
    m.insert(1, 0, 0, &b"a"[..]);
    let rd = RegionDownload::new(Arc::new(m), spec(world(), 1, 1)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let rep = rd
        .download(dir.path().join("s.pmtiles"), &mut |_| true)
        .await
        .unwrap();
    assert_eq!((rep.tiles_written, rep.tiles_missing), (1, 3));
}

#[test]
fn invalid_zoom() {
    let b = BBox::new(0.0, 0.0, 1.0, 1.0).unwrap();
    assert!(RegionDownload::new(Arc::new(MemorySource::new("x")), spec(b, 5, 2)).is_err());
}
