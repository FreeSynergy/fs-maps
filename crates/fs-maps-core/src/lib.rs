//! `fs-maps-core`: the GUI-free core of FreeSynergy maps.
//!
//! * [`TileSource`]: tiles on demand, one at a time (remote PMTiles via HTTP range requests,
//!   local PMTiles files, fallback chains).
//! * [`TileCache`]: size-limited on-disk LRU cache of raw MVT tiles.
//! * [`RegionDownload`]: bbox + zoom range -> local PMTiles package for offline use.
//! * [`places`]: GeoJSON `Feature` <-> `fs-store` objects (AS2 `Place`); [`geouri`]: RFC 5870.
//!
//! See `DESIGN.md` and `RESEARCH.md` (usage policies!).

pub mod cache;
pub mod error;
pub mod geouri;
pub mod places;
pub mod region;
pub mod source;
#[doc(hidden)]
pub mod testutil;
pub mod tile;

pub use cache::{CacheConfig, CacheStats, CachedSource, TileCache};
pub use error::{Error, Result};
pub use region::{Estimate, Progress, RegionDownload, RegionReport, RegionSpec};
pub use source::{
    FallbackSource, MemorySource, PmtilesFileSource, PmtilesHttpSource, SourceInfo, TileSource,
};
pub use tile::{BBox, Tile};
