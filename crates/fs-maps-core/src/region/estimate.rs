//! Size estimation before a download.
//!
//! Pattern: Strategy consumer — a pure function over a plan and a `dyn TileSource`; counts
//! exactly and samples a few tiles evenly spread over the region.

use super::plan::{TilePlan, coord_of};
use super::spec::Estimate;
use crate::error::Result;
use crate::source::TileSource;

/// Exact tile count plus a size guess from at most `sample` fetched tiles (`0` = count only).
pub(super) async fn estimate(
    source: &dyn TileSource,
    plan: &TilePlan,
    sample: usize,
) -> Result<Estimate> {
    let tiles = plan.total();
    let per_zoom = plan.counts().to_vec();
    if sample == 0 || tiles == 0 {
        return Ok(Estimate {
            tiles,
            tiles_per_zoom: per_zoom,
            sampled: 0,
            avg_tile_bytes: 0,
            est_bytes: 0,
        });
    }
    let ids = plan.sorted_ids()?;
    let n = sample.min(ids.len());
    let mut bytes = 0u64;
    for i in 0..n {
        let c = coord_of(ids[i * ids.len() / n])?;
        if let Some(t) = source.get_tile(c.z(), c.x(), c.y()).await? {
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
