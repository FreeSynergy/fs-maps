//! Tile sources: where tiles come from, one tile at a time.
//!
//! Pattern: Strategy — [`TileSource`] is the interface; PMTiles file/HTTP, memory, fallback chain
//! and the cache decorator are interchangeable implementations.

mod fallback;
mod file;
mod http;
mod memory;
mod pmt;
mod policy;
mod tile_source;

pub use fallback::FallbackSource;
pub use file::PmtilesFileSource;
pub use http::PmtilesHttpSource;
pub use memory::MemorySource;
pub use tile_source::{SourceInfo, TileSource};
