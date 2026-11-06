//! Web-Mercator tile geometry: [`Tile`] coordinates and [`BBox`] regions.
//!
//! Pattern: Newtype — validated value types (a `Tile` or `BBox` that exists is in range).

mod bbox;
mod coord;

pub use bbox::BBox;
pub use coord::{MAX_LAT, MAX_ZOOM, Tile};
