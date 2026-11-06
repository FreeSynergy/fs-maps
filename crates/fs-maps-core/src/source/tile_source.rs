//! The [`TileSource`] interface.
//!
//! Pattern: Strategy — callers (cache, region download, UI) depend on this trait only, so the
//! origin of tiles (remote archive, local file, memory, chain) can change freely. It is the
//! provider interface `TileSource` of `places.capnp`; its one capability, `tile.read`, is what
//! `get_tile` does, so every source reports it (ADR 0045).

use async_trait::async_trait;
use bytes::Bytes;
use fs_interfaces::Capabilities;

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
    /// What this source can do: `tile.read`. A source that cannot serve tiles overrides this and
    /// says why.
    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new(format!("tiles:{}", self.id()));
        caps.supported("tile.read");
        caps
    }
}
