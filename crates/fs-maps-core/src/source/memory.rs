//! In-memory tile source (tests, tiny built-in maps).
//!
//! Pattern: Strategy implementation — the simplest [`TileSource`], a map from coordinate to bytes.

use std::collections::HashMap;

use async_trait::async_trait;
use bytes::Bytes;

use super::TileSource;
use crate::error::Result;
use crate::tile::Tile;

/// In-memory source.
#[derive(Default)]
pub struct MemorySource {
    id: String,
    tiles: HashMap<(u8, u32, u32), Bytes>,
}

impl MemorySource {
    /// Empty source with the given id.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            tiles: HashMap::new(),
        }
    }

    /// Add or replace a tile.
    pub fn insert(&mut self, z: u8, x: u32, y: u32, data: impl Into<Bytes>) {
        self.tiles.insert((z, x, y), data.into());
    }
}

#[async_trait]
impl TileSource for MemorySource {
    fn id(&self) -> String {
        self.id.clone()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        Tile::new(z, x, y)?;
        Ok(self.tiles.get(&(z, x, y)).cloned())
    }
}
