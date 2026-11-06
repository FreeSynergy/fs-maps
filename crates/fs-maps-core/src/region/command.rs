//! The download as a command object.
//!
//! Pattern: Command — `DownloadCommand` captures everything needed (source, region, plan,
//! target) and `run` executes it, reporting to a `ProgressObserver`. The partial file is removed
//! on error or cancel; on success it is renamed into place.

use std::fs::File;
use std::path::Path;

use pmtiles::{PmTilesWriter, TileType};
use serde_json::json;

use super::observer::ProgressObserver;
use super::plan::TilePlan;
use super::spec::{Progress, RegionReport, RegionSpec};
use crate::error::{Error, Result};
use crate::source::TileSource;

/// One download run.
pub(super) struct DownloadCommand<'a> {
    pub source: &'a dyn TileSource,
    pub spec: &'a RegionSpec,
    pub plan: &'a TilePlan,
}

impl DownloadCommand<'_> {
    pub async fn run(
        &self,
        path: &Path,
        observer: &mut dyn ProgressObserver,
    ) -> Result<RegionReport> {
        let (lo, hi) = self
            .plan
            .zooms()
            .unwrap_or((self.spec.min_zoom, self.spec.max_zoom));
        let b = self.spec.bbox;
        let meta = json!({
            "name": self.spec.name,
            "attribution": self.spec.attribution,
            "source": self.source.id(),
            "bounds": [b.min_lon, b.min_lat, b.max_lon, b.max_lat],
            "minzoom": lo, "maxzoom": hi,
            "generator": "fs-maps-core",
        });
        let coords = self.plan.coords()?;
        let part = path.with_extension("pmtiles.part");
        let file = File::create(&part).map_err(|e| Error::io(&part, e))?;
        let mut w = PmTilesWriter::new(TileType::Mvt)
            .min_zoom(lo)
            .max_zoom(hi)
            .bounds(b.min_lon, b.min_lat, b.max_lon, b.max_lat)
            .center(
                f64::midpoint(b.min_lon, b.max_lon),
                f64::midpoint(b.min_lat, b.max_lat),
            )
            .center_zoom(lo.max(hi.min(lo + (hi - lo) / 2)))
            .metadata(meta.to_string())
            .create(file)?;

        let mut p = Progress {
            done: 0,
            total: self.plan.total(),
            missing: 0,
            bytes: 0,
        };
        let mut written = 0u64;
        for c in coords {
            let c = c?;
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
            if !observer.on_progress(p) {
                drop(w);
                let _ = std::fs::remove_file(&part);
                return Err(Error::Cancelled);
            }
        }
        w.finalize()?;
        std::fs::rename(&part, path).map_err(|e| Error::io(path, e))?;
        let file_bytes = std::fs::metadata(path)
            .map_err(|e| Error::io(path, e))?
            .len();
        Ok(RegionReport {
            path: path.to_path_buf(),
            tiles_written: written,
            tiles_missing: p.missing,
            payload_bytes: p.bytes,
            file_bytes,
        })
    }
}
