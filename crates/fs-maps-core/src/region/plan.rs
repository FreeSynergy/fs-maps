//! The set of tiles a region needs.
//!
//! Pattern: Iterator — the plan yields tile coordinates in TileID (Hilbert) order, which is the
//! order a clustered PMTiles archive must be written in.

use pmtiles::{TileCoord, TileId};

use super::spec::RegionSpec;
use crate::error::{Error, Result};
use crate::source::SourceInfo;
use crate::tile::{BBox, Tile};

/// Zoom levels, per-zoom counts and the sorted tile ids of a region.
pub(super) struct TilePlan {
    bbox: BBox,
    zooms: Option<(u8, u8)>,
    counts: Vec<(u8, u64)>,
}

impl TilePlan {
    /// Clamp the zoom range to what the source offers and count tiles; refuses more than
    /// `max_tiles`.
    pub fn new(spec: &RegionSpec, info: Option<SourceInfo>, max_tiles: u64) -> Result<Self> {
        let (mut lo, mut hi) = (spec.min_zoom, spec.max_zoom);
        if let Some(i) = info {
            lo = lo.max(i.min_zoom);
            hi = hi.min(i.max_zoom);
        }
        let zooms = (lo <= hi).then_some((lo, hi));
        let mut counts = Vec::new();
        let mut total = 0u64;
        for z in zooms.map_or(0..0, |(lo, hi)| lo..hi + 1) {
            let n = spec.bbox.tile_count(z)?;
            total += n;
            if total > max_tiles {
                return Err(Error::TooManyTiles {
                    tiles: total,
                    limit: max_tiles,
                });
            }
            counts.push((z, n));
        }
        Ok(Self {
            bbox: spec.bbox,
            zooms,
            counts,
        })
    }

    pub fn zooms(&self) -> Option<(u8, u8)> {
        self.zooms
    }

    pub fn counts(&self) -> &[(u8, u64)] {
        &self.counts
    }

    pub fn total(&self) -> u64 {
        self.counts.iter().map(|c| c.1).sum()
    }

    /// All tile ids, sorted.
    pub fn sorted_ids(&self) -> Result<Vec<u64>> {
        let mut ids = Vec::with_capacity(self.total() as usize);
        for &(z, _) in &self.counts {
            let ((x0, x1), (y0, y1)) = self.bbox.tile_range(z)?;
            for x in x0..=x1 {
                for y in y0..=y1 {
                    ids.push(TileId::from(Tile::new(z, x, y)?.pmt()?).value());
                }
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    /// Tile coordinates in Hilbert order.
    pub fn coords(&self) -> Result<impl Iterator<Item = Result<TileCoord>>> {
        Ok(self.sorted_ids()?.into_iter().map(coord_of))
    }
}

pub(super) fn coord_of(id: u64) -> Result<TileCoord> {
    Ok(TileCoord::from(TileId::new(id)?))
}
