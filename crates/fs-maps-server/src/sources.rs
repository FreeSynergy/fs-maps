//! Which tiles a program serves, from its configuration.
//!
//! Pattern: Factory + Chain of Responsibility — local packages first, in the order given, then the
//! remote archive through the cache. Nothing configured is the explicit `NoSource`. The configuration
//! comes from the environment, because the shared `ServerOptions` take no arguments of a program.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs_interfaces::rpc::prepare_data_dir;
use fs_maps_core::cache::source_hash;
use fs_maps_core::{
    CacheConfig, CachedSource, Error, FallbackSource, LazyHttpSource, NoSource, PmtilesFileSource,
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

const FILES: &str = "FS_MAPS_TILES";
const URL: &str = "FS_MAPS_TILES_URL";
const USER_AGENT: &str = "FS_MAPS_USER_AGENT";
const CACHE: &str = "FS_MAPS_CACHE";
const CACHE_MIB: &str = "FS_MAPS_CACHE_MIB";
const DEFAULT_CACHE_MIB: u64 = 1024;
const DEFAULT_USER_AGENT: &str = concat!(
    "FreeSynergy-Maps/",
    env!("CARGO_PKG_VERSION"),
    " (+https://freesynergy.net)"
);

fn set(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

fn text(name: &str) -> Result<Option<String>, String> {
    set(name)
        .map(|v| v.into_string().map_err(|_| format!("{name} is not text")))
        .transpose()
}

/// An absolute path from the environment; the XDG rule ignores a relative one.
fn absolute(name: &str) -> Option<PathBuf> {
    set(name).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// Two data directories are two people's or two purposes' maps: one default cache each.
fn default_cache(data: &Path) -> Option<PathBuf> {
    let base = absolute("XDG_CACHE_HOME").or_else(|| absolute("HOME").map(|h| h.join(".cache")))?;
    let data = std::path::absolute(data).unwrap_or_else(|_| data.to_path_buf());
    let name = format!("{:016x}", source_hash(&data.to_string_lossy()));
    Some(base.join("freesynergy").join("maps").join(name))
}

fn cache_of_env(data: &Path) -> Result<CacheConfig, String> {
    let mib = match text(CACHE_MIB)? {
        Some(v) => v
            .parse::<u64>()
            .map_err(|_| format!("{CACHE_MIB} needs a number"))?,
        None => DEFAULT_CACHE_MIB,
    };
    let dir = set(CACHE)
        .map(PathBuf::from)
        .or_else(|| default_cache(data))
        .ok_or_else(|| format!("no $XDG_CACHE_HOME or $HOME: set {CACHE}"))?;
    Ok(CacheConfig::new(dir, mib.saturating_mul(1024 * 1024)))
}

impl TileConfig {
    /// `FS_MAPS_TILES`: local `PMTiles` files, a path list like `$PATH`. `FS_MAPS_TILES_URL`: a remote
    /// archive, read through a cache (`FS_MAPS_CACHE`, default below `$XDG_CACHE_HOME/freesynergy/maps`,
    /// one for each data directory; `FS_MAPS_CACHE_MIB`, default 1024); `FS_MAPS_USER_AGENT` names
    /// the program to the host.
    ///
    /// # Errors
    /// A value that cannot be read, or no place for the cache.
    pub fn from_env(data: &Path) -> Result<Self, String> {
        let files = set(FILES)
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        let url = text(URL)?;
        let cache = url.is_some().then(|| cache_of_env(data)).transpose()?;
        let user_agent = text(USER_AGENT)?.unwrap_or_else(|| DEFAULT_USER_AGENT.into());
        Ok(Self {
            files,
            url,
            user_agent,
            cache,
        })
    }
}

fn remote(url: &str, config: &TileConfig) -> fs_maps_core::Result<Arc<dyn TileSource>> {
    let source = LazyHttpSource::new(url, &config.user_agent)?;
    Ok(match &config.cache {
        Some(cache) => {
            // It shows where a person looked: the same rule as the data.
            prepare_data_dir(&cache.dir).map_err(|source| Error::Io {
                path: cache.dir.clone(),
                source,
            })?;
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
pub async fn open_tiles(config: &TileConfig) -> fs_maps_core::Result<Arc<dyn TileSource>> {
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
