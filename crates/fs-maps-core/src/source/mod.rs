//! Tile sources: where tiles come from, one tile at a time.
//!
//! Pattern: Strategy — [`TileSource`] is the interface; PMTiles file/HTTP, memory, fallback chain
//! and the cache decorator are interchangeable implementations.

mod decode;
mod directories;
mod fallback;
mod file;
mod guard;
mod http;
mod http_backend;
mod lazy;
mod memory;
mod none;
mod pmt;
mod policy;
mod tile_source;

pub use decode::MAX_TILE_BYTES;
pub use fallback::FallbackSource;
pub use file::PmtilesFileSource;
pub use http::PmtilesHttpSource;
pub use lazy::LazyHttpSource;
pub use memory::MemorySource;
pub use none::NoSource;
pub use tile_source::{SourceInfo, TileSource};
