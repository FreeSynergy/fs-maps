//! Fallback chain of tile sources.
//!
//! Pattern: Chain of Responsibility — each source in order may answer; the first that has the
//! tile wins, the rest are never asked. A tile that cannot exist reaches nobody.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

use super::{SourceInfo, TileSource};
use crate::error::{Error, Result};
use crate::tile::Tile;

/// First source that has the tile wins. Typical chain: offline region packages, then
/// `CachedSource(PmtilesHttpSource)`. A failing source is skipped if a later one has the tile;
/// the first error is returned only if nobody has it.
pub struct FallbackSource {
    sources: Vec<Arc<dyn TileSource>>,
}

impl FallbackSource {
    /// Chain in priority order.
    pub fn new(sources: Vec<Arc<dyn TileSource>>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl TileSource for FallbackSource {
    fn id(&self) -> String {
        let ids: Vec<_> = self.sources.iter().map(|s| s.id()).collect();
        format!("fallback[{}]", ids.join(","))
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        Tile::new(z, x, y)?;
        let mut first_err = None;
        for s in &self.sources {
            match s.get_tile(z, x, y).await {
                Ok(Some(t)) => return Ok(Some(t)),
                Ok(None) => {}
                Err(e @ Error::InvalidTile { .. }) => return Err(e),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        first_err.map_or(Ok(None), Err)
    }
    fn info(&self) -> Option<SourceInfo> {
        self.sources
            .iter()
            .filter_map(|s| s.info())
            .reduce(|a, b| SourceInfo {
                min_zoom: a.min_zoom.min(b.min_zoom),
                max_zoom: a.max_zoom.max(b.max_zoom),
                bounds: None,
            })
    }
}
