//! Read-through cache around any tile source.
//!
//! Pattern: Decorator — implements `TileSource` itself and wraps another one: hit -> cache,
//! miss -> inner source, then store. A tile stays until a fetch replaced it, also when expired.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

use super::TileCache;
use crate::error::{Error, Result};
use crate::source::{MAX_TILE_BYTES, SourceInfo, TileSource};
use crate::tile::Tile;

/// A source that reads through a [`TileCache`].
pub struct CachedSource<S> {
    inner: S,
    cache: Arc<TileCache>,
}

impl<S: TileSource> CachedSource<S> {
    /// Wrap `inner`.
    pub fn new(inner: S, cache: Arc<TileCache>) -> Self {
        Self { inner, cache }
    }

    /// The cache in use.
    pub fn cache(&self) -> &TileCache {
        &self.cache
    }

    /// The wrapped source.
    pub fn inner(&self) -> &S {
        &self.inner
    }
}

impl<S: TileSource> CachedSource<S> {
    /// The key of the data as far as known. Offline it is the last version seen, so what is cached
    /// still answers; versions never mix.
    fn key(&self) -> String {
        let id = self.inner.cache_id();
        let version = match self.inner.version() {
            Some(version) => {
                self.cache.remember_version(&id, &version);
                Some(version)
            }
            None => self.cache.known_version(&id),
        };
        match version {
            Some(version) => format!("{id}#{version}"),
            None => id,
        }
    }
}

#[async_trait]
impl<S: TileSource> TileSource for CachedSource<S> {
    fn id(&self) -> String {
        self.inner.id()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        Tile::new(z, x, y)?;
        // Offline this fails, and what is cached still answers below.
        let _ = self.inner.refresh().await;
        let key = self.key();
        if let Some(t) = self.cache.get(&key, z, x, y) {
            return Ok(Some(t));
        }
        match self.inner.get_tile(z, x, y).await {
            Ok(Some(t)) => {
                // Only under the version it was asked under: the archive may have changed while this
                // read was on its way, and these bytes may be of either version. A failing cache
                // write must not fail the read.
                if t.len() <= MAX_TILE_BYTES && self.key() == key {
                    let _ = self.cache.put(&key, z, x, y, &t);
                }
                Ok(Some(t))
            }
            Ok(None) => {
                self.cache.forget(&key, z, x, y);
                Ok(None)
            }
            // An expired tile is better than none while the source is away.
            Err(e) if matches!(e, Error::TileTooLarge { .. }) => Err(e),
            Err(e) => self
                .cache
                .get_stale(&key, z, x, y)
                .map_or(Err(e), |t| Ok(Some(t))),
        }
    }
    fn info(&self) -> Option<SourceInfo> {
        self.inner.info()
    }
    fn capabilities(&self) -> fs_interfaces::Capabilities {
        self.inner.capabilities()
    }
    fn cache_id(&self) -> String {
        self.inner.cache_id()
    }
    fn version(&self) -> Option<String> {
        self.inner.version()
    }
    async fn refresh(&self) -> Result<()> {
        self.inner.refresh().await
    }
}
