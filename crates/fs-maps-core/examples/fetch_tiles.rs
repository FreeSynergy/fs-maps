//! Fetch a few tiles of a small area, cache them, show cache stats.
//!
//! Without arguments: generates a `PMTiles` fixture and serves it on 127.0.0.1 (no public server is
//! contacted). With a URL: `cargo run --example fetch_tiles -- https://your.host/map.pmtiles` --
//! only use a server whose operator allows it; this example issues about a dozen requests.

use std::sync::Arc;

use fs_maps_core::testutil::{RangeServer, write_fixture};
use fs_maps_core::{
    BBox, CacheConfig, CachedSource, PmtilesHttpSource, RegionDownload, RegionSpec, Tile,
    TileCache, TileSource,
};

const UA: &str = "FreeSynergy-Maps-example/0.1 (+https://github.com/FreeSynergy; local test)";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = std::env::temp_dir().join(format!("fs-maps-example-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let (url, _server) = if let Some(u) = std::env::args().nth(1) {
        (u, None)
    } else {
        let f = tmp.join("fixture.pmtiles");
        write_fixture(&f, 8).await?;
        let s = RangeServer::start(&f).await?;
        println!("serving fixture on {}", s.url);
        (s.url.clone(), Some(s))
    };

    let source = PmtilesHttpSource::open(&url, UA).await?;
    println!("source info: {:?}", source.info());
    // 16 KiB cache: small on purpose, to show eviction
    let cache = Arc::new(TileCache::open(CacheConfig::new(
        tmp.join("cache"),
        16 * 1024,
    ))?);
    let cached = CachedSource::new(source, cache.clone());

    // the area: central Berlin, zoom 8..=8 plus neighbours
    let centre = Tile::from_lon_lat(8, 13.405, 52.52)?;
    for round in 1..=2 {
        for dx in 0..3u32 {
            for dy in 0..3u32 {
                let (x, y) = (centre.x - 1 + dx, centre.y - 1 + dy);
                let t = cached.get_tile(8, x, y).await?;
                if round == 1 && dx == 0 && dy == 0 {
                    println!("tile 8/{x}/{y}: {} bytes", t.map_or(0, |t| t.len()));
                }
            }
        }
        println!("after round {round}: {:?}", cache.stats());
    }

    // a region package for offline use, with size estimate first
    let spec = RegionSpec {
        name: "Berlin (demo)".into(),
        bbox: BBox::new(13.0, 52.3, 13.8, 52.7)?,
        min_zoom: 0,
        max_zoom: 8,
        attribution: Some("© OpenStreetMap contributors".into()),
    };
    let rd = RegionDownload::new(Arc::new(PmtilesHttpSource::open(&url, UA).await?), spec)?;
    let est = rd.estimate(4).await?;
    println!(
        "region: {} tiles, ~{} bytes uncompressed",
        est.tiles, est.est_bytes
    );
    let out = tmp.join("berlin.pmtiles");
    let rep = rd
        .download(&out, &mut |p| {
            if p.done % 4 == 0 || p.done == p.total {
                println!("  {}/{} tiles", p.done, p.total);
            }
            true
        })
        .await?;
    println!(
        "package \"{}\": {} tiles, {} bytes on disk",
        rep.path.display(),
        rep.tiles_written,
        rep.file_bytes
    );
    std::fs::remove_dir_all(&tmp)?;
    Ok(())
}
