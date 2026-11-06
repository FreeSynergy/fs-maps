//! Which tiles a program serves, from its configuration.
//!
//! Pattern: Factory + Chain of Responsibility — local packages first, in the order given, then the
//! remote archive through the cache. Nothing configured is the explicit `NoSource`.

use std::path::PathBuf;
use std::sync::Arc;

use fs_maps_core::{
    CacheConfig, CachedSource, FallbackSource, LazyHttpSource, NoSource, PmtilesFileSource, Result,
    TileCache, TileSource,
};

/// Where tiles come from.
#[derive(Clone, Debug, Default)]
pub struct TileConfig {
    /// Local `PMTiles` files, for example region packages; asked first.
    pub files: Vec<PathBuf>,
    /// A remote `PMTiles` archive, opened at the first tile that needs it.
    pub url: Option<String>,
    /// The User-Agent of the remote requests (mandatory for a URL, see the usage policy).
    pub user_agent: String,
    /// The cache in front of the remote archive.
    pub cache: Option<CacheConfig>,
}

fn remote(url: &str, config: &TileConfig) -> Result<Arc<dyn TileSource>> {
    let source = LazyHttpSource::new(url, &config.user_agent)?;
    Ok(match &config.cache {
        Some(cache) => {
            let cache = Arc::new(TileCache::open(cache.clone())?);
            Arc::new(CachedSource::new(source, cache))
        }
        None => Arc::new(source),
    })
}

/// Open what `config` names.
///
/// # Errors
/// A file cannot be read as `PMTiles`, the usage policy forbids the URL, or the cache cannot be opened.
/// An unreachable URL is not an error here: the program also works offline.
pub async fn open_tiles(config: &TileConfig) -> Result<Arc<dyn TileSource>> {
    let mut chain: Vec<Arc<dyn TileSource>> = Vec::new();
    for file in &config.files {
        chain.push(Arc::new(PmtilesFileSource::open(file).await?));
    }
    if let Some(url) = &config.url {
        chain.push(remote(url, config)?);
    }
    Ok(match chain.len() {
        0 => Arc::new(NoSource),
        1 => chain.remove(0),
        _ => Arc::new(FallbackSource::new(chain)),
    })
}
