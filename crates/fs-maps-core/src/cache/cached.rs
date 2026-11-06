//! Read-through cache around any tile source.
//!
//! Pattern: Decorator — implements `TileSource` itself and wraps another one: hit -> cache,
//! miss -> inner source, then store.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;

use super::TileCache;
use crate::error::Result;
use crate::source::{SourceInfo, TileSource};

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

#[async_trait]
impl<S: TileSource> TileSource for CachedSource<S> {
    fn id(&self) -> String {
        self.inner.id()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        let id = self.inner.id();
        if let Some(t) = self.cache.get(&id, z, x, y) {
            return Ok(Some(t));
        }
        let t = self.inner.get_tile(z, x, y).await?;
        if let Some(t) = &t {
            // a failing cache write must not fail the read
            let _ = self.cache.put(&id, z, x, y, t);
        }
        Ok(t)
    }
    fn info(&self) -> Option<SourceInfo> {
        self.inner.info()
    }
    fn capabilities(&self) -> fs_interfaces::Capabilities {
        self.inner.capabilities()
    }
}
