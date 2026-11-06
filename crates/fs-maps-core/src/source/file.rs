//! Local PMTiles file as a tile source.
//!
//! Pattern: Adapter — an archive over the file backend, behind [`TileSource`].

use std::path::Path;

use async_trait::async_trait;
use bytes::Bytes;
use pmtiles::FileBackend;

use super::pmt::{Archive, IN_FLIGHT};
use super::{SourceInfo, TileSource};
use crate::error::Result;

/// A local PMTiles file (an offline region package, or any downloaded PMTiles).
pub struct PmtilesFileSource {
    id: String,
    archive: Archive<FileBackend>,
}

impl PmtilesFileSource {
    /// Open a PMTiles file of MVT tiles; anything else is refused here.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let backend = FileBackend::try_from(path).await?;
        Ok(Self {
            id: format!("file:{}", path.display()),
            archive: Archive::open(backend, IN_FLIGHT).await?,
        })
    }
}

#[async_trait]
impl TileSource for PmtilesFileSource {
    fn id(&self) -> String {
        self.id.clone()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        self.archive.tile(z, x, y).await
    }
    fn info(&self) -> Option<SourceInfo> {
        Some(self.archive.info())
    }
}
