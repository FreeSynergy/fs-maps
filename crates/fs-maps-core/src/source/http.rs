//! Remote PMTiles archive as a tile source.
//!
//! Pattern: Adapter — an archive over the HTTP backend, behind [`TileSource`], with the usage policy
//! applied at construction.

use async_trait::async_trait;
use bytes::Bytes;
use pmtiles::reqwest::{Client, Url};

use super::http_backend::{HttpBackend, http_client};
use super::pmt::{Archive, IN_FLIGHT};
use super::policy::{check_url, public_id};
use super::{SourceInfo, TileSource};
use crate::error::Result;

/// A remote PMTiles archive read with HTTP range requests (header + root directory first, then
/// leaf directories and tiles on demand). Directories are cached in memory, within a budget.
pub struct PmtilesHttpSource {
    url: Url,
    archive: Archive<HttpBackend>,
}

impl PmtilesHttpSource {
    /// `user_agent` is mandatory and must identify the application (e.g.
    /// `"FreeSynergy-Maps/0.1 (+https://example.org; contact: ops@example.org)"`); library
    /// defaults are rejected, as the OSMF policy requires. At most 4 requests are in flight.
    pub async fn open(url: &str, user_agent: &str) -> Result<Self> {
        Self::open_with(url, user_agent, IN_FLIGHT).await
    }

    /// Like [`PmtilesHttpSource::open`] with a custom in-flight request limit.
    pub async fn open_with(url: &str, user_agent: &str, max_in_flight: usize) -> Result<Self> {
        let client = http_client(user_agent)?;
        Self::connect(client, check_url(url)?, max_in_flight).await
    }

    pub(super) async fn connect(client: Client, url: Url, max_in_flight: usize) -> Result<Self> {
        let backend = HttpBackend {
            client,
            url: url.clone(),
        };
        Ok(Self {
            url,
            archive: Archive::open(backend, max_in_flight).await?,
        })
    }
}

#[async_trait]
impl TileSource for PmtilesHttpSource {
    fn id(&self) -> String {
        public_id(&self.url)
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        self.archive.tile(z, x, y).await
    }
    fn info(&self) -> Option<SourceInfo> {
        Some(self.archive.info())
    }
    fn cache_id(&self) -> String {
        self.url.to_string()
    }
    fn version(&self) -> Option<String> {
        self.archive.version()
    }
}
