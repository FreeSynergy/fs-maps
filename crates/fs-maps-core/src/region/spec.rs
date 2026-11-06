//! Plain data types of the region download.
//!
//! Pattern: Value objects — immutable-ish descriptions of a region and the results about it.

use std::path::PathBuf;

use crate::tile::BBox;

/// What to download.
#[derive(Clone, Debug)]
pub struct RegionSpec {
    /// Human name, written into the package metadata.
    pub name: String,
    /// Geographic extent.
    pub bbox: BBox,
    /// Lowest zoom (inclusive).
    pub min_zoom: u8,
    /// Highest zoom (inclusive).
    pub max_zoom: u8,
    /// Written into the package metadata, e.g. "© OpenStreetMap contributors".
    pub attribution: Option<String>,
}

/// Tile count and size estimate, before downloading.
#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    /// Exact number of tiles in the pyramid.
    pub tiles: u64,
    /// Tiles per zoom level.
    pub tiles_per_zoom: Vec<(u8, u64)>,
    /// Number of tiles actually fetched to measure the average size.
    pub sampled: usize,
    /// Average size of a sampled tile (missing ones count as 0).
    pub avg_tile_bytes: u64,
    /// Estimated size of the **uncompressed** MVT payload; the gzip PMTiles file is usually smaller.
    pub est_bytes: u64,
}

/// Progress after one tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Tiles processed so far.
    pub done: u64,
    /// Tiles in total.
    pub total: u64,
    /// Tiles the source did not have.
    pub missing: u64,
    /// Payload bytes written so far.
    pub bytes: u64,
}

/// Result of a finished download.
#[derive(Clone, Debug)]
pub struct RegionReport {
    /// The finished package.
    pub path: PathBuf,
    /// Tiles written.
    pub tiles_written: u64,
    /// Tiles the source did not have.
    pub tiles_missing: u64,
    /// Sum of tile payload sizes (uncompressed).
    pub payload_bytes: u64,
    /// Size of the package file.
    pub file_bytes: u64,
}
