//! A remote archive opened on first use.
//!
//! Pattern: Proxy — stands in for a [`PmtilesHttpSource`] until the network answers, so a program
//! starts while offline and its local packages and cache keep working. The usage policy is still
//! checked at construction: a forbidden configuration fails at once. A failed open is tried again
//! at the next tile.

use async_trait::async_trait;
use bytes::Bytes;
use pmtiles::reqwest::Url;
use tokio::sync::OnceCell;

use super::policy::{check_url, check_user_agent};
use super::{PmtilesHttpSource, SourceInfo, TileSource};
use crate::error::Result;

/// A [`PmtilesHttpSource`] that opens when the first tile is asked for.
pub struct LazyHttpSource {
    url: Url,
    user_agent: String,
    opened: OnceCell<PmtilesHttpSource>,
}

impl LazyHttpSource {
    /// Check the policy (see [`PmtilesHttpSource::open`]) without sending a request.
    pub fn new(url: &str, user_agent: &str) -> Result<Self> {
        check_user_agent(user_agent)?;
        Ok(Self {
            url: check_url(url)?,
            user_agent: user_agent.to_owned(),
            opened: OnceCell::new(),
        })
    }
}

#[async_trait]
impl TileSource for LazyHttpSource {
    fn id(&self) -> String {
        self.url.to_string()
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        let source = self
            .opened
            .get_or_try_init(|| PmtilesHttpSource::open(self.url.as_str(), &self.user_agent))
            .await?;
        source.get_tile(z, x, y).await
    }
    fn info(&self) -> Option<SourceInfo> {
        self.opened.get().and_then(TileSource::info)
    }
}
