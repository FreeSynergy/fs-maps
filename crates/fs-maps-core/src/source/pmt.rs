//! Shared glue between the `pmtiles` crate and [`TileSource`](super::TileSource).
//!
//! Pattern: Adapter — translates the foreign header/reader types into our `SourceInfo` and
//! tile reads.

use bytes::Bytes;
use pmtiles::{AsyncPmTilesReader, TileId};

use super::SourceInfo;
use crate::error::Result;
use crate::tile::{BBox, Tile};

pub(super) fn info_of(h: &pmtiles::Header) -> SourceInfo {
    SourceInfo {
        min_zoom: h.min_zoom,
        max_zoom: h.max_zoom,
        bounds: BBox::new(
            h.min_longitude,
            h.min_latitude,
            h.max_longitude,
            h.max_latitude,
        )
        .ok(),
    }
}

pub(super) async fn read_tile<B, C>(
    r: &AsyncPmTilesReader<B, C>,
    z: u8,
    x: u32,
    y: u32,
) -> Result<Option<Bytes>>
where
    B: pmtiles::AsyncBackend + Sync + Send,
    C: pmtiles::DirectoryCache + Sync + Send,
{
    let coord = Tile::new(z, x, y)?.pmt()?;
    Ok(r.get_tile_decompressed(TileId::from(coord)).await?)
}
