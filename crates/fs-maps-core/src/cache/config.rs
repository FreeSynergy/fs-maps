//! Cache configuration.
//!
//! Pattern: Builder-style value object — defaults via `new`, tweak public fields afterwards.

use std::path::PathBuf;
use std::time::Duration;

/// Where and how large the cache is.
#[derive(Clone, Debug)]
pub struct CacheConfig {
    /// Cache directory (created on open).
    pub dir: PathBuf,
    /// Hard limit for the sum of all file sizes (including the 8-byte headers).
    pub max_bytes: u64,
    /// Entries older than this are treated as missing. `None` = never expire.
    pub ttl: Option<Duration>,
}

impl CacheConfig {
    /// Config with the default 7-day TTL.
    pub fn new(dir: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Self {
            dir: dir.into(),
            max_bytes,
            ttl: Some(Duration::from_secs(7 * 24 * 3600)),
        }
    }
}
