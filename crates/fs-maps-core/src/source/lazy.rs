//! A remote archive opened on first use, and again when it changed.
//!
//! Pattern: Proxy + State — stands in for a [`PmtilesHttpSource`] until the network answers, so a
//! program starts while offline and its local packages and cache keep working. The usage policy and
//! the client are checked at construction: a forbidden configuration fails at once. An open that
//! failed is remembered for a while, so a silent host does not make every tile wait. An archive that
//! was replaced is opened again, once per tile that notices.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use pmtiles::PmtError;
use pmtiles::reqwest::{Client, Url};

use super::http_backend::http_client;
use super::pmt::IN_FLIGHT;
use super::policy::{check_url, public_id};
use super::{PmtilesHttpSource, SourceInfo, TileSource};
use crate::error::{Error, Result};
use crate::tile::Tile;

const OPEN_TIMEOUT: Duration = Duration::from_secs(8);
const BACKOFF: Duration = Duration::from_secs(30);

/// A [`PmtilesHttpSource`] that opens when the first tile is asked for.
pub struct LazyHttpSource {
    url: Url,
    client: Client,
    open_timeout: Duration,
    backoff: Duration,
    current: Mutex<Option<Arc<PmtilesHttpSource>>>,
    last_failure: tokio::sync::Mutex<Option<(Instant, String)>>,
}

impl LazyHttpSource {
    /// Check the policy (see [`PmtilesHttpSource::open`]) and build the client, without a request.
    pub fn new(url: &str, user_agent: &str) -> Result<Self> {
        Ok(Self {
            client: http_client(user_agent)?,
            url: check_url(url)?,
            open_timeout: OPEN_TIMEOUT,
            backoff: BACKOFF,
            current: Mutex::new(None),
            last_failure: tokio::sync::Mutex::new(None),
        })
    }

    /// How long an open may take, and how long its failure is remembered.
    #[must_use]
    pub fn with_timing(mut self, open_timeout: Duration, backoff: Duration) -> Self {
        self.open_timeout = open_timeout;
        self.backoff = backoff;
        self
    }

    fn current(&self) -> Option<Arc<PmtilesHttpSource>> {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn replace(&self, source: Option<Arc<PmtilesHttpSource>>) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = source;
    }

    /// The open archive. Callers wait for one open at a time, then share its outcome.
    async fn opened(&self) -> Result<Arc<PmtilesHttpSource>> {
        if let Some(source) = self.current() {
            return Ok(source);
        }
        let mut failure = self.last_failure.lock().await;
        if let Some(source) = self.current() {
            return Ok(source);
        }
        if let Some((at, why)) = failure.as_ref()
            && at.elapsed() < self.backoff
        {
            return Err(Error::SourceUnavailable(why.clone()));
        }
        let open = PmtilesHttpSource::connect(self.client.clone(), self.url.clone(), IN_FLIGHT);
        let outcome = match tokio::time::timeout(self.open_timeout, open).await {
            Ok(outcome) => outcome,
            Err(_) => Err(Error::SourceUnavailable("the host did not answer".into())),
        };
        match outcome {
            Ok(source) => {
                let source = Arc::new(source);
                self.replace(Some(source.clone()));
                *failure = None;
                Ok(source)
            }
            Err(e) => {
                *failure = Some((Instant::now(), e.to_string()));
                Err(e)
            }
        }
    }

    /// Forget this reader, unless another caller replaced it already.
    fn outdated(&self, source: &Arc<PmtilesHttpSource>) {
        if self.current().is_some_and(|c| Arc::ptr_eq(&c, source)) {
            self.replace(None);
        }
    }
}

#[async_trait]
impl TileSource for LazyHttpSource {
    fn id(&self) -> String {
        public_id(&self.url)
    }
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        Tile::new(z, x, y)?;
        let source = self.opened().await?;
        match source.get_tile(z, x, y).await {
            Err(Error::Pmtiles(PmtError::SourceModified)) => {
                self.outdated(&source);
                self.opened().await?.get_tile(z, x, y).await
            }
            other => other,
        }
    }
    fn info(&self) -> Option<SourceInfo> {
        self.current().and_then(|source| source.info())
    }
    fn cache_id(&self) -> String {
        self.url.to_string()
    }
    fn version(&self) -> Option<String> {
        self.current().and_then(|source| source.version())
    }
    async fn refresh(&self) -> Result<()> {
        self.opened().await.map(|_| ())
    }
}
