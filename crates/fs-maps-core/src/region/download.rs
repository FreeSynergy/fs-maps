//! Entry point of the region download.
//!
//! Pattern: Facade — validates the region, builds the plan and hands it to estimate/command.

use std::path::Path;
use std::sync::Arc;

use super::command::DownloadCommand;
use super::estimate::estimate;
use super::observer::{FnObserver, ProgressObserver};
use super::plan::TilePlan;
use super::spec::{Estimate, Progress, RegionReport, RegionSpec};
use crate::error::{Error, Result};
use crate::source::TileSource;
use crate::tile::MAX_ZOOM;

/// Download a bbox + zoom range from a [`TileSource`] into a PMTiles package.
pub struct RegionDownload {
    source: Arc<dyn TileSource>,
    spec: RegionSpec,
    max_tiles: u64,
}

impl RegionDownload {
    /// Default safety limit: 200 000 tiles.
    pub const DEFAULT_MAX_TILES: u64 = 200_000;

    /// Validate the zoom range and set up a download.
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

    /// Change the tile limit.
    #[must_use]
    pub fn with_max_tiles(mut self, max: u64) -> Self {
        self.max_tiles = max;
        self
    }

    fn plan(&self) -> Result<TilePlan> {
        TilePlan::new(&self.spec, self.source.info(), self.max_tiles)
    }

    /// Tile count (exact) and size estimate, before downloading. Fetches at most `sample`
    /// tiles (spread evenly over the region) to measure the average size; `sample = 0` skips
    /// all requests and only counts.
    pub async fn estimate(&self, sample: usize) -> Result<Estimate> {
        estimate(self.source.as_ref(), &self.plan()?, sample).await
    }

    /// Download into `path` (a `.pmtiles` file). `progress` is called after every tile; return
    /// `false` from it to cancel (the partial file is removed and `Error::Cancelled` returned).
    /// The file is written to `<path>.part` and renamed on success.
    pub async fn download(
        &self,
        path: impl AsRef<Path>,
        progress: &mut dyn FnMut(Progress) -> bool,
    ) -> Result<RegionReport> {
        self.download_observed(path, &mut FnObserver(progress))
            .await
    }

    /// Like [`RegionDownload::download`] with a [`ProgressObserver`] instead of a closure.
    pub async fn download_observed(
        &self,
        path: impl AsRef<Path>,
        observer: &mut dyn ProgressObserver,
    ) -> Result<RegionReport> {
        let plan = self.plan()?;
        DownloadCommand {
            source: self.source.as_ref(),
            spec: &self.spec,
            plan: &plan,
        }
        .run(path.as_ref(), observer)
        .await
    }
}
