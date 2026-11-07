//! Bounding boxes as store index keys, and the search area of `within`.
//!
//! Pattern: Specification — [`Area`] is a validated search box that turns itself into store
//! conditions. The store compares index values as binary text, so a coordinate is written as a
//! fixed-width decimal of 1e-7 degrees (about 1 cm), offset to be positive: text order is
//! numeric order. Stored boxes are rounded outwards (minimum down, maximum up), query boxes the
//! same way, so overlap is closed and never misses a place; it may add one that is 1e-7 degrees
//! away. A place overlaps an area when the bounding box of its geometry does (not the geometry).

use fs_objects::{Cond, IndexFields, Op};

use crate::error::{Error, Result};
use crate::tile::BBox;

/// Index key: western edge of the geometry's bounding box.
pub const LON_MIN: &str = "lon_min";
/// Index key: eastern edge.
pub const LON_MAX: &str = "lon_max";
/// Index key: southern edge.
pub const LAT_MIN: &str = "lat_min";
/// Index key: northern edge.
pub const LAT_MAX: &str = "lat_max";

const SCALE: f64 = 1e7;

/// Sortable key of `degrees` (`offset` 180 for longitude, 90 for latitude), rounded down or up.
fn key(degrees: f64, offset: f64, up: bool) -> String {
    let v = (degrees + offset) * SCALE;
    let v = if up { v.ceil() } else { v.floor() };
    format!("{:010}", v.max(0.0) as u64)
}

/// Index fields that make a place findable by `within`: the four edges of its bounding box.
pub fn bbox_index(b: &BBox) -> IndexFields {
    let one = |k: &str, v: String| (k.to_string(), vec![v]);
    [
        one(LON_MIN, key(b.min_lon, 180.0, false)),
        one(LON_MAX, key(b.max_lon, 180.0, true)),
        one(LAT_MIN, key(b.min_lat, 90.0, false)),
        one(LAT_MAX, key(b.max_lat, 90.0, true)),
    ]
    .into_iter()
    .collect()
}

/// A search box as in `places.capnp` (`BBox`): `west > east` means it crosses the antimeridian.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    /// Western edge, -180..=180.
    pub west: f64,
    /// Southern edge, -90..=90, not above `north`.
    pub south: f64,
    /// Eastern edge, -180..=180. Less than `west` for a box over the antimeridian.
    pub east: f64,
    /// Northern edge, -90..=90.
    pub north: f64,
}

impl Area {
    /// Validated area (finite, in WGS84 range, `south <= north`).
    pub fn new(west: f64, south: f64, east: f64, north: f64) -> Result<Self> {
        let ok = [west, south, east, north].iter().all(|v| v.is_finite())
            && (-180.0..=180.0).contains(&west)
            && (-180.0..=180.0).contains(&east)
            && (-90.0..=90.0).contains(&south)
            && (-90.0..=90.0).contains(&north)
            && south <= north;
        if ok {
            Ok(Self {
                west,
                south,
                east,
                north,
            })
        } else {
            Err(Error::InvalidBBox(format!("{west},{south},{east},{north}")))
        }
    }

    /// Whether the area crosses the antimeridian.
    pub fn crosses_antimeridian(&self) -> bool {
        self.west > self.east
    }

    /// The store conditions: `(all of these, at least one of these)`.
    ///
    /// Latitude and the normal longitude test are ANDed. Over the antimeridian the area is
    /// `[west, 180]` plus `[-180, east]`; a box `[a, b]` meets one of them when `b >= west` or
    /// `a <= east`, which is a single OR-group, so it is still one store query.
    pub fn conditions(&self) -> (Vec<Cond>, Vec<Cond>) {
        let at = |k: &str, op, v| Cond::Index {
            key: k.into(),
            op,
            value: v,
        };
        let mut all = vec![
            at(LAT_MIN, Op::Le, key(self.north, 90.0, true)),
            at(LAT_MAX, Op::Ge, key(self.south, 90.0, false)),
        ];
        let lon_max_ge_west = at(LON_MAX, Op::Ge, key(self.west, 180.0, false));
        let lon_min_le_east = at(LON_MIN, Op::Le, key(self.east, 180.0, true));
        if self.crosses_antimeridian() {
            return (all, vec![lon_max_ge_west, lon_min_le_east]);
        }
        all.extend([lon_max_ge_west, lon_min_le_east]);
        (all, vec![])
    }
}
