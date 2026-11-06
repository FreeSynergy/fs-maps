//! Remote PMTiles archive as a tile source.
//!
//! Pattern: Adapter — `AsyncPmTilesReader<HttpBackend>` behind [`TileSource`], with the usage
//! policy applied at construction and a semaphore bounding requests in flight.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use pmtiles::reqwest::{Client, Url};
use pmtiles::{AsyncPmTilesReader, HashMapCache, HttpBackend};
use tokio::sync::Semaphore;

use super::pmt::{info_of, read_tile};
use super::policy::{check_url, check_user_agent};
use super::{SourceInfo, TileSource};
use crate::error::Result;

/// A remote PMTiles archive read with HTTP range requests (header + root directory first, then
/// leaf directories and tiles on demand). Directories are cached in memory by the reader.
pub struct PmtilesHttpSource {
    url: Url,
    reader: AsyncPmTilesReader<HttpBackend, HashMapCache>,
    limit: Arc<Semaphore>,
}

impl PmtilesHttpSource {
    /// `user_agent` is mandatory and must identify the application (e.g.
    /// `"FreeSynergy-Maps/0.1 (+https://example.org; contact: ops@example.org)"`); library
    /// defaults are rejected, as the OSMF policy requires. At most 4 requests are in flight.
    pub async fn open(url: &str, user_agent: &str) -> Result<Self> {
        Self::open_with(url, user_agent, 4).await
    }

    /// Like [`PmtilesHttpSource::open`] with a custom in-flight request limit.
    pub async fn open_with(url: &str, user_agent: &str, max_in_flight: usize) -> Result<Self> {
        check_user_agent(user_agent)?;
        let url = check_url(url)?;
        let client = Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(30))
            .build()?;
        let reader = AsyncPmTilesReader::new_with_url_and_cache(
            client,
            url.clone(),
            HashMapCache::default(),
        )
        .await?;
        Ok(Self {
            url,
            reader,
            limit: Arc::new(Semaphore::new(max_in_flight.max(1))),
        })
    }
}

#[async_trait]
impl TileSource for PmtilesHttpSource {
    fn id(&self) -> String {
        self.url.to_string()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        let _permit = self.limit.acquire().await.expect("semaphore never closed");
        read_tile(&self.reader, z, x, y).await
    }
    fn info(&self) -> Option<SourceInfo> {
        Some(info_of(self.reader.get_header()))
    }
}
