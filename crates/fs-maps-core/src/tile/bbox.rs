//! WGS84 bounding box and its tile coverage.
//!
//! Pattern: Newtype — a validated box (ranges, ordering) with tile-range maths.

use super::coord::Tile;
use crate::error::{Error, Result};

/// WGS84 bounding box (`min_lon, min_lat, max_lon, max_lat`). Antimeridian crossing is not supported
/// (split into two boxes).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BBox {
    /// Western edge.
    pub min_lon: f64,
    /// Southern edge.
    pub min_lat: f64,
    /// Eastern edge.
    pub max_lon: f64,
    /// Northern edge.
    pub max_lat: f64,
}

impl BBox {
    /// Validated constructor (finite, in WGS84 range, `min <= max`).
    pub fn new(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Result<Self> {
        let ok = [min_lon, min_lat, max_lon, max_lat]
            .iter()
            .all(|v| v.is_finite())
            && (-180.0..=180.0).contains(&min_lon)
            && (-180.0..=180.0).contains(&max_lon)
            && (-90.0..=90.0).contains(&min_lat)
            && (-90.0..=90.0).contains(&max_lat)
            && min_lon <= max_lon
            && min_lat <= max_lat;
        if !ok {
            return Err(Error::InvalidBBox(format!(
                "{min_lon},{min_lat},{max_lon},{max_lat}"
            )));
        }
        Ok(Self {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        })
    }

    /// Inclusive tile ranges `(x0..=x1, y0..=y1)` covering the box at zoom `z`.
    pub fn tile_range(&self, z: u8) -> Result<((u32, u32), (u32, u32))> {
        let nw = Tile::from_lon_lat(z, self.min_lon, self.max_lat)?;
        let se = Tile::from_lon_lat(z, self.max_lon, self.min_lat)?;
        Ok(((nw.x, se.x), (nw.y, se.y)))
    }

    /// Number of tiles covering the box at zoom `z`.
    pub fn tile_count(&self, z: u8) -> Result<u64> {
        let ((x0, x1), (y0, y1)) = self.tile_range(z)?;
        Ok((u64::from(x1 - x0) + 1) * (u64::from(y1 - y0) + 1))
    }

    /// Whether the two boxes overlap.
    pub fn intersects(&self, other: &BBox) -> bool {
        self.min_lon <= other.max_lon
            && self.max_lon >= other.min_lon
            && self.min_lat <= other.max_lat
            && self.max_lat >= other.min_lat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts() {
        let world = BBox::new(-180.0, -90.0, 180.0, 90.0).unwrap();
        assert_eq!(world.tile_count(0).unwrap(), 1);
        assert_eq!(world.tile_count(3).unwrap(), 64);
        let b = BBox::new(13.0, 52.3, 13.8, 52.7).unwrap();
        assert!(b.tile_count(10).unwrap() >= 1);
        assert!(b.tile_count(12).unwrap() > b.tile_count(10).unwrap());
    }
}
