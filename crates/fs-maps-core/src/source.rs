//! Tile sources: where tiles come from, one tile at a time.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use pmtiles::reqwest::{Client, Url};
use pmtiles::{
    AsyncPmTilesReader, FileBackend, HashMapCache, HttpBackend, NoCache, TileCoord, TileId,
};
use tokio::sync::Semaphore;

use crate::error::{Error, Result};
use crate::tile::{BBox, Tile};

/// What a source knows about itself (from the PMTiles header).
#[derive(Clone, Debug, PartialEq)]
pub struct SourceInfo {
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub bounds: Option<BBox>,
}

/// A provider of vector tiles. Returned bytes are **uncompressed MVT**.
#[async_trait]
pub trait TileSource: Send + Sync {
    /// Stable identity, used as cache key part (a URL or a path).
    fn id(&self) -> String;
    /// `Ok(None)` = the source has no such tile (not an error).
    async fn get_tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>>;
    /// Zoom range / bounds, when known.
    fn info(&self) -> Option<SourceInfo> {
        None
    }
}

fn info_of(h: &pmtiles::Header) -> SourceInfo {
    SourceInfo {
        min_zoom: h.min_zoom,
        max_zoom: h.max_zoom,
        bounds: BBox::new(h.min_longitude, h.min_latitude, h.max_longitude, h.max_latitude).ok(),
    }
}

async fn read_tile<B, C>(r: &AsyncPmTilesReader<B, C>, z: u8, x: u32, y: u32) -> Result<Option<Bytes>>
where
    B: pmtiles::AsyncBackend + Sync + Send,
    C: pmtiles::DirectoryCache + Sync + Send,
{
    let coord = Tile::new(z, x, y)?.pmt()?;
    Ok(r.get_tile_decompressed(TileId::from(coord)).await?)
}

/// A local PMTiles file (an offline region package, or any downloaded PMTiles).
pub struct PmtilesFileSource {
    id: String,
    reader: AsyncPmTilesReader<FileBackend, NoCache>,
}

impl PmtilesFileSource {
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

/// Hosts for which tile requests are refused: the OSMF raster tile servers forbid bulk/offline
/// use and are not PMTiles anyway (see RESEARCH.md section 1).
const FORBIDDEN_HOSTS: &[&str] = &["tile.openstreetmap.org", "tile.osm.org"];

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

    pub async fn open_with(url: &str, user_agent: &str, max_in_flight: usize) -> Result<Self> {
        check_user_agent(user_agent)?;
        let url = Url::parse(url).map_err(|e| Error::Policy(format!("bad URL: {e}")))?;
        if !matches!(url.scheme(), "https" | "http") {
            return Err(Error::Policy(format!("unsupported scheme {}", url.scheme())));
        }
        if let Some(host) = url.host_str() {
            let host = host.to_ascii_lowercase();
            if FORBIDDEN_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
                return Err(Error::Policy(format!(
                    "{host}: OSMF tile servers forbid offline/prefetch use; use a PMTiles source"
                )));
            }
        }
        let client = Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(30))
            .build()?;
        let reader =
            AsyncPmTilesReader::new_with_url_and_cache(client, url.clone(), HashMapCache::default())
                .await?;
        Ok(Self {
            url,
            reader,
            limit: Arc::new(Semaphore::new(max_in_flight.max(1))),
        })
    }
}

fn check_user_agent(ua: &str) -> Result<()> {
    let l = ua.trim().to_ascii_lowercase();
    let generic = ["reqwest", "curl/", "python-requests", "okhttp", "go-http-client", "java/"];
    if l.len() < 5 || generic.iter().any(|g| l.starts_with(g)) {
        return Err(Error::Policy(
            "a clear application User-Agent is required (no library defaults)".into(),
        ));
    }
    Ok(())
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

/// In-memory source (tests, tiny built-in maps).
#[derive(Default)]
pub struct MemorySource {
    id: String,
    tiles: HashMap<(u8, u32, u32), Bytes>,
}

impl MemorySource {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            tiles: HashMap::new(),
        }
    }
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

/// First source that has the tile wins. Typical chain: offline region packages, then
/// `CachedSource(PmtilesHttpSource)`. A failing source is skipped if a later one has the tile;
/// the first error is returned only if nobody has it.
pub struct FallbackSource {
    sources: Vec<Arc<dyn TileSource>>,
}

impl FallbackSource {
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
        let mut first_err = None;
        for s in &self.sources {
            match s.get_tile(z, x, y).await {
                Ok(Some(t)) => return Ok(Some(t)),
                Ok(None) => {}
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        first_err.map_or(Ok(None), Err)
    }
    fn info(&self) -> Option<SourceInfo> {
        self.sources.iter().filter_map(|s| s.info()).reduce(|a, b| SourceInfo {
            min_zoom: a.min_zoom.min(b.min_zoom),
            max_zoom: a.max_zoom.max(b.max_zoom),
            bounds: None,
        })
    }
}

#[allow(dead_code)]
fn _assert_coord(_: TileCoord) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_rules() {
        assert!(check_user_agent("").is_err());
        assert!(check_user_agent("reqwest/0.13").is_err());
        assert!(check_user_agent("curl/8.0").is_err());
        assert!(check_user_agent("FreeSynergy-Maps/0.1 (+https://example.org)").is_ok());
    }

    #[tokio::test]
    async fn osmf_tile_server_refused_before_any_request() {
        let ua = "FreeSynergy-Maps/0.1 (test)";
        for u in [
            "https://tile.openstreetmap.org/0/0/0.png",
            "https://a.tile.openstreetmap.org/x.pmtiles",
        ] {
            assert!(matches!(PmtilesHttpSource::open(u, ua).await, Err(Error::Policy(_))), "{u}");
        }
        assert!(matches!(
            PmtilesHttpSource::open("https://example.org/a.pmtiles", "reqwest/1").await,
            Err(Error::Policy(_))
        ));
        assert!(matches!(
            PmtilesHttpSource::open("ftp://example.org/a.pmtiles", ua).await,
            Err(Error::Policy(_))
        ));
    }

    #[tokio::test]
    async fn fallback_order() {
        let mut a = MemorySource::new("a");
        a.insert(1, 0, 0, &b"A"[..]);
        let mut b = MemorySource::new("b");
        b.insert(1, 0, 0, &b"B"[..]);
        b.insert(1, 1, 1, &b"B2"[..]);
        let f = FallbackSource::new(vec![Arc::new(a), Arc::new(b)]);
        assert_eq!(f.get_tile(1, 0, 0).await.unwrap().unwrap(), &b"A"[..]);
        assert_eq!(f.get_tile(1, 1, 1).await.unwrap().unwrap(), &b"B2"[..]);
        assert!(f.get_tile(1, 0, 1).await.unwrap().is_none());
    }
}
