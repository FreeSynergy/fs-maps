//! Web-Mercator tile maths (XYZ scheme, y down, as in OSM and PMTiles).

use std::f64::consts::PI;

use crate::error::{Error, Result};

/// Highest zoom level we accept (PMTiles allows 31; real data stops far below).
pub const MAX_ZOOM: u8 = 22;
/// Web-Mercator latitude limit.
pub const MAX_LAT: f64 = 85.051_128_779_806_59;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tile {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl Tile {
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

/// WGS84 bounding box (`min_lon, min_lat, max_lon, max_lat`). Antimeridian crossing is not supported
/// (split into two boxes).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BBox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl BBox {
    pub fn new(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Result<Self> {
        let ok = [min_lon, min_lat, max_lon, max_lat].iter().all(|v| v.is_finite())
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
    fn known_tiles() {
        // Berlin Brandenburg Gate at z10 / z0
        assert_eq!(Tile::from_lon_lat(0, 13.4, 52.5).unwrap(), Tile { z: 0, x: 0, y: 0 });
        assert_eq!(
            Tile::from_lon_lat(10, 13.3777, 52.5163).unwrap(),
            Tile { z: 10, x: 550, y: 335 }
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
