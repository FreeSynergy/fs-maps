//! Offline regions: download all tiles of a bbox + zoom range into a local PMTiles file.
//!
//! The package is a normal PMTiles v3 archive (MVT tiles, gzip), written with the `pmtiles`
//! crate's writer, so it can be read by [`crate::PmtilesFileSource`], by any PMTiles tool, and
//! shared as a file or through Git (ADR 0036). Tiles are fetched strictly one after another
//! (polite to servers) and written in Hilbert/TileID order (clustered archive).
//!
//! Never point this at `tile.openstreetmap.org` (forbidden; see RESEARCH.md). Use a PMTiles
//! source whose operator allows it (your own hosting, or a Protomaps build copy).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pmtiles::{PmTilesWriter, TileId, TileType};
use serde_json::json;

use crate::error::{Error, Result};
use crate::source::TileSource;
use crate::tile::{BBox, MAX_ZOOM, Tile};

#[derive(Clone, Debug)]
pub struct RegionSpec {
    pub name: String,
    pub bbox: BBox,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// Written into the package metadata, e.g. "© OpenStreetMap contributors".
    pub attribution: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    pub tiles: u64,
    pub tiles_per_zoom: Vec<(u8, u64)>,
    /// Number of tiles actually fetched to measure the average size.
    pub sampled: usize,
    pub avg_tile_bytes: u64,
    /// Estimated size of the **uncompressed** MVT payload; the gzip PMTiles file is usually smaller.
    pub est_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    pub missing: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct RegionReport {
    pub path: PathBuf,
    pub tiles_written: u64,
    pub tiles_missing: u64,
    pub payload_bytes: u64,
    pub file_bytes: u64,
}

pub struct RegionDownload {
    source: Arc<dyn TileSource>,
    spec: RegionSpec,
    max_tiles: u64,
}

impl RegionDownload {
    /// Default safety limit: 200 000 tiles.
    pub const DEFAULT_MAX_TILES: u64 = 200_000;

    pub fn new(source: Arc<dyn TileSource>, spec: RegionSpec) -> Result<Self> {
        if spec.min_zoom > spec.max_zoom || spec.max_zoom > MAX_ZOOM {
            return Err(Error::InvalidZoom {
                min: spec.min_zoom,
                max: spec.max_zoom,
            });
        }
        Ok(Self {
            source,
            spec,
            max_tiles: Self::DEFAULT_MAX_TILES,
        })
    }

    pub fn with_max_tiles(mut self, max: u64) -> Self {
        self.max_tiles = max;
        self
    }

    /// Zoom range after clamping to what the source offers.
    fn zooms(&self) -> Option<(u8, u8)> {
        let (mut lo, mut hi) = (self.spec.min_zoom, self.spec.max_zoom);
        if let Some(i) = self.source.info() {
            lo = lo.max(i.min_zoom);
            hi = hi.min(i.max_zoom);
        }
        (lo <= hi).then_some((lo, hi))
    }

    fn counts(&self) -> Result<Vec<(u8, u64)>> {
        let Some((lo, hi)) = self.zooms() else {
            return Ok(vec![]);
        };
        let mut v = Vec::new();
        let mut total = 0u64;
        for z in lo..=hi {
            let n = self.spec.bbox.tile_count(z)?;
            total += n;
            if total > self.max_tiles {
                return Err(Error::TooManyTiles {
                    tiles: total,
                    limit: self.max_tiles,
                });
            }
            v.push((z, n));
        }
        Ok(v)
    }

    /// All tiles in TileID (Hilbert) order.
    fn tile_ids(&self) -> Result<Vec<u64>> {
        let counts = self.counts()?;
        let mut ids = Vec::with_capacity(counts.iter().map(|c| c.1).sum::<u64>() as usize);
        for (z, _) in counts {
            let ((x0, x1), (y0, y1)) = self.spec.bbox.tile_range(z)?;
            for x in x0..=x1 {
                for y in y0..=y1 {
                    ids.push(TileId::from(Tile::new(z, x, y)?.pmt()?).value());
                }
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    /// Tile count (exact) and size estimate, before downloading. Fetches at most `sample`
    /// tiles (spread evenly over the region) to measure the average size; `sample = 0` skips
    /// all requests and only counts.
    pub async fn estimate(&self, sample: usize) -> Result<Estimate> {
        let per_zoom = self.counts()?;
        let tiles: u64 = per_zoom.iter().map(|c| c.1).sum();
        if sample == 0 || tiles == 0 {
            return Ok(Estimate {
                tiles,
                tiles_per_zoom: per_zoom,
                sampled: 0,
                avg_tile_bytes: 0,
                est_bytes: 0,
            });
        }
        let ids = self.tile_ids()?;
        let n = sample.min(ids.len());
        let mut bytes = 0u64;
        for i in 0..n {
            let id = ids[i * ids.len() / n];
            let c = pmtiles::TileCoord::from(TileId::new(id)?);
            if let Some(t) = self.source.get_tile(c.z(), c.x(), c.y()).await? {
                bytes += t.len() as u64;
            }
        }
        let avg = bytes / n as u64;
        Ok(Estimate {
            tiles,
            tiles_per_zoom: per_zoom,
            sampled: n,
            avg_tile_bytes: avg,
            est_bytes: avg * tiles,
        })
    }

    /// Download into `path` (a `.pmtiles` file). `progress` is called after every tile; return
    /// `false` from it to cancel (the partial file is removed and `Error::Cancelled` returned).
    /// The file is written to `<path>.part` and renamed on success.
    pub async fn download(
        &self,
        path: impl AsRef<Path>,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<RegionReport> {
        let path = path.as_ref();
        let ids = self.tile_ids()?;
        let (lo, hi) = self.zooms().unwrap_or((self.spec.min_zoom, self.spec.max_zoom));
        let b = self.spec.bbox;
        let meta = json!({
            "name": self.spec.name,
            "attribution": self.spec.attribution,
            "source": self.source.id(),
            "bounds": [b.min_lon, b.min_lat, b.max_lon, b.max_lat],
            "minzoom": lo, "maxzoom": hi,
            "generator": "fs-maps-core",
        });
        let part = path.with_extension("pmtiles.part");
        let file = File::create(&part).map_err(|e| Error::io(&part, e))?;
        let mut w = PmTilesWriter::new(TileType::Mvt)
            .min_zoom(lo)
            .max_zoom(hi)
            .bounds(b.min_lon, b.min_lat, b.max_lon, b.max_lat)
            .center((b.min_lon + b.max_lon) / 2.0, (b.min_lat + b.max_lat) / 2.0)
            .center_zoom(lo.max(hi.min(lo + (hi - lo) / 2)))
            .metadata(meta.to_string())
            .create(file)?;

        let total = ids.len() as u64;
        let mut p = Progress { done: 0, total, missing: 0, bytes: 0 };
        let mut written = 0u64;
        for id in ids {
            let c = pmtiles::TileCoord::from(TileId::new(id)?);
            match self.source.get_tile(c.z(), c.x(), c.y()).await {
                Ok(Some(t)) => {
                    p.bytes += t.len() as u64;
                    w.add_tile(c, &t)?;
                    written += 1;
                }
                Ok(None) => p.missing += 1,
                Err(e) => {
                    drop(w);
                    let _ = std::fs::remove_file(&part);
                    return Err(e);
                }
            }
            p.done += 1;
            if !progress(p) {
                drop(w);
                let _ = std::fs::remove_file(&part);
                return Err(Error::Cancelled);
            }
        }
        w.finalize()?;
        std::fs::rename(&part, path).map_err(|e| Error::io(path, e))?;
        let file_bytes = std::fs::metadata(path).map_err(|e| Error::io(path, e))?.len();
        Ok(RegionReport {
            path: path.to_path_buf(),
            tiles_written: written,
            tiles_missing: p.missing,
            payload_bytes: p.bytes,
            file_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{MemorySource, PmtilesFileSource};

    /// Source with a tile for every coordinate of zoom 0..=4 whose payload encodes the coordinate.
    fn full_source() -> MemorySource {
        let mut m = MemorySource::new("mem:full");
        for z in 0..=4u8 {
            for x in 0..(1u32 << z) {
                for y in 0..(1u32 << z) {
                    m.insert(z, x, y, format!("{z}/{x}/{y}-{}", "x".repeat(20)).into_bytes());
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

    #[tokio::test]
    async fn estimate_counts_without_requests() {
        let b = BBox::new(-180.0, -85.0, 180.0, 85.0).unwrap();
        let rd = RegionDownload::new(Arc::new(full_source()), spec(b, 0, 3)).unwrap();
        let e = rd.estimate(0).await.unwrap();
        assert_eq!(e.tiles, 1 + 4 + 16 + 64);
        assert_eq!(e.sampled, 0);
        let e = rd.estimate(10).await.unwrap();
        assert_eq!(e.sampled, 10);
        assert!(e.avg_tile_bytes > 20 && e.est_bytes == e.avg_tile_bytes * e.tiles);
    }

    #[tokio::test]
    async fn too_many_tiles_refused() {
        let b = BBox::new(-180.0, -85.0, 180.0, 85.0).unwrap();
        let rd = RegionDownload::new(Arc::new(full_source()), spec(b, 0, 4))
            .unwrap()
            .with_max_tiles(50);
        assert!(matches!(rd.estimate(0).await, Err(Error::TooManyTiles { .. })));
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
        let b = BBox::new(-180.0, -85.0, 180.0, 85.0).unwrap();
        let rd = RegionDownload::new(Arc::new(full_source()), spec(b, 0, 3)).unwrap();
        let r = rd.download(&out, &mut |p| p.done < 5).await;
        assert!(matches!(r, Err(Error::Cancelled)));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn zoom_clamped_to_source_and_missing_counted() {
        let mut m = MemorySource::new("sparse");
        m.insert(1, 0, 0, &b"a"[..]);
        let b = BBox::new(-180.0, -85.0, 180.0, 85.0).unwrap();
        let rd = RegionDownload::new(Arc::new(m), spec(b, 1, 1)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let rep = rd.download(dir.path().join("s.pmtiles"), &mut |_| true).await.unwrap();
        assert_eq!((rep.tiles_written, rep.tiles_missing), (1, 3));
    }

    #[test]
    fn invalid_zoom() {
        let b = BBox::new(0.0, 0.0, 1.0, 1.0).unwrap();
        assert!(RegionDownload::new(Arc::new(MemorySource::new("x")), spec(b, 5, 2)).is_err());
    }
}
