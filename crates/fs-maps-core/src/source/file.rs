//! Local PMTiles file as a tile source.
//!
//! Pattern: Adapter — `AsyncPmTilesReader<FileBackend>` behind [`TileSource`].

use std::path::Path;

use async_trait::async_trait;
use bytes::Bytes;
use pmtiles::{AsyncPmTilesReader, FileBackend, NoCache};

use super::pmt::{info_of, read_tile};
use super::{SourceInfo, TileSource};
use crate::error::Result;

/// A local PMTiles file (an offline region package, or any downloaded PMTiles).
pub struct PmtilesFileSource {
    id: String,
    reader: AsyncPmTilesReader<FileBackend, NoCache>,
}

impl PmtilesFileSource {
    /// Open a PMTiles file.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let reader = AsyncPmTilesReader::new_with_path(path).await?;
        Ok(Self {
            id: format!("file:{}", path.display()),
            reader,
        })
    }
}

#[async_trait]
impl TileSource for PmtilesFileSource {
    fn id(&self) -> String {
        self.id.clone()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        read_tile(&self.reader, z, x, y).await
    }
    fn info(&self) -> Option<SourceInfo> {
        Some(info_of(self.reader.get_header()))
    }
}
