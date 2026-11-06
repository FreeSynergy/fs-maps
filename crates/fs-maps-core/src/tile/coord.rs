//! Web-Mercator tile coordinate (XYZ scheme, y down, as in OSM and PMTiles).
//!
//! Pattern: Newtype — a validated `z/x/y` that cannot be out of range once constructed.

use std::f64::consts::PI;

use crate::error::{Error, Result};

/// Highest zoom level we accept (PMTiles allows 31; real data stops far below).
pub const MAX_ZOOM: u8 = 22;
/// Web-Mercator latitude limit.
pub const MAX_LAT: f64 = 85.051_128_779_806_59;

/// A tile coordinate `z/x/y`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tile {
    /// Zoom level.
    pub z: u8,
    /// Column (west to east).
    pub x: u32,
    /// Row (north to south).
    pub y: u32,
}

impl Tile {
    /// Validated constructor: `z <= MAX_ZOOM`, `x`/`y` below `2^z`.
    pub fn new(z: u8, x: u32, y: u32) -> Result<Self> {
        if z > MAX_ZOOM || x >= (1u32 << z) || y >= (1u32 << z) {
            return Err(Error::InvalidTile { z, x, y });
        }
        Ok(Self { z, x, y })
    }

    /// The tile containing a WGS84 point at zoom `z` (latitude clamped to Web-Mercator range).
    pub fn from_lon_lat(z: u8, lon: f64, lat: f64) -> Result<Self> {
        let n = (1u64 << z) as f64;
        let lat = lat.clamp(-MAX_LAT, MAX_LAT);
        let x = ((lon + 180.0) / 360.0 * n).floor();
        let y = ((1.0 - (lat.to_radians().tan() + 1.0 / lat.to_radians().cos()).ln() / PI) / 2.0
            * n)
            .floor();
        let max = n - 1.0;
        Self::new(z, x.clamp(0.0, max) as u32, y.clamp(0.0, max) as u32)
    }

    pub(crate) fn pmt(self) -> Result<pmtiles::TileCoord> {
        Ok(pmtiles::TileCoord::new(self.z, self.x, self.y)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tile::BBox;

    #[test]
    fn known_tiles() {
        // Berlin Brandenburg Gate at z10 / z0
        assert_eq!(
            Tile::from_lon_lat(0, 13.4, 52.5).unwrap(),
            Tile { z: 0, x: 0, y: 0 }
        );
        assert_eq!(
            Tile::from_lon_lat(10, 13.3777, 52.5163).unwrap(),
            Tile {
                z: 10,
                x: 550,
                y: 335
            }
        );
        assert_eq!(
            Tile::from_lon_lat(1, -179.0, 80.0).unwrap(),
            Tile { z: 1, x: 0, y: 0 }
        );
    }

    #[test]
    fn rejects_bad_tiles_and_boxes() {
        assert!(Tile::new(1, 2, 0).is_err());
        assert!(Tile::new(23, 0, 0).is_err());
        assert!(BBox::new(10.0, 50.0, 9.0, 51.0).is_err());
        assert!(BBox::new(0.0, -91.0, 1.0, 1.0).is_err());
        assert!(BBox::new(f64::NAN, 0.0, 1.0, 1.0).is_err());
    }
}
