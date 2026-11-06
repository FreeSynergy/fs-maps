//! The [`TileSource`] interface.
//!
//! Pattern: Strategy — callers (cache, region download, UI) depend on this trait only, so the
//! origin of tiles (remote archive, local file, memory, chain) can change freely.

use async_trait::async_trait;
use bytes::Bytes;

use crate::error::Result;
use crate::tile::BBox;

/// What a source knows about itself (from the PMTiles header).
#[derive(Clone, Debug, PartialEq)]
pub struct SourceInfo {
    /// Lowest zoom with data.
    pub min_zoom: u8,
    /// Highest zoom with data.
    pub max_zoom: u8,
    /// Geographic bounds, when known.
    pub bounds: Option<BBox>,
}

/// A provider of vector tiles. Returned bytes are **uncompressed MVT**.
#[async_trait]
pub trait TileSource: Send + Sync {
    /// Stable identity, used as cache key part (a URL or a path).
    fn id(&self) -> String;
    /// `Ok(None)` = the source has no such tile (not an error).
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>>;
    /// Zoom range / bounds, when known.
    fn info(&self) -> Option<SourceInfo> {
        None
    }
}
