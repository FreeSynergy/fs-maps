//! `fs-maps-core`: the GUI-free core of FreeSynergy maps.
//!
//! Pattern: Strategy at the centre — [`TileSource`] with Adapters (PMTiles file/HTTP), a
//! Decorator ([`CachedSource`]) and a Chain of Responsibility ([`FallbackSource`]); region
//! download is a Command with an Observer; places are an Adapter GeoJSON <-> store.
//!
//! * [`TileSource`]: tiles on demand, one at a time.
//! * [`TileCache`]: size-limited on-disk LRU cache of raw MVT tiles.
//! * [`RegionDownload`]: bbox + zoom range -> local PMTiles package for offline use.
//! * [`places`]: GeoJSON `Feature` <-> `fs-store` objects (AS2 `Place`); [`geouri`]: RFC 5870.
//!
//! See `DESIGN.md` and `RESEARCH.md` (usage policies!).

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// Pedantic lints that only add noise here: error/panic docs on every fn (lock `expect`s cannot
// fail), `#[must_use]` on every getter, proper nouns in docs (PMTiles, GeoJSON), z/x/y names are
// the tile-scheme convention, exact float compares are intentional (pole check), and numeric
// casts in tile maths happen after range validation.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::doc_markdown,
    clippy::many_single_char_names,
    clippy::float_cmp,
    clippy::must_use_candidate,
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless
)]

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
pub use fs_interfaces::{Capabilities, Unsupported};
pub use places::{
    Area, Paging, PlaceChange, PlaceFeed, PlaceListener, PlaceRepository, Places, StorePlaces,
    StoreView, Subscription,
};
pub use region::{Estimate, Progress, ProgressObserver, RegionDownload, RegionReport, RegionSpec};
pub use source::{
    FallbackSource, LazyHttpSource, MAX_TILE_BYTES, MemorySource, NoSource, PmtilesFileSource,
    PmtilesHttpSource, SourceInfo, TileSource,
};
pub use tile::{BBox, Tile};
