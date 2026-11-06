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
    /// Stable identity (a URL or a path), shown to every consumer: without credentials.
    fn id(&self) -> String;
    /// `Ok(None)` = the source has no such tile (not an error).
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>>;
    /// Zoom range / bounds, when known.
    fn info(&self) -> Option<SourceInfo> {
        None
    }
    /// The identity the cache keys by. Not shown to anyone, so unlike [`TileSource::id`] it may name
    /// all of a URL.
    fn cache_id(&self) -> String {
        self.id()
    }
    /// The version of the data behind the source, once known (`ETag`, `Last-Modified`).
    fn version(&self) -> Option<String> {
        None
    }
    /// Learn the version if it is not known yet. Failing is no failure of a tile: the cache may hold it.
    async fn refresh(&self) -> Result<()> {
        Ok(())
    }
    /// What this source can do: `tile.read`. A source that cannot serve tiles overrides this and
    /// says why.
    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new(format!("tiles:{}", self.id()));
        caps.supported("tile.read");
        caps
    }
}
