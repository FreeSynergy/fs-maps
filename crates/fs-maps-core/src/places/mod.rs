//! Places: GeoJSON (RFC 7946) `Feature` <-> `fs-store` object of AS2 type `Place`.
//!
//! Pattern: Adapter (+ Repository on top) — `adapter` converts, `geometry`/`name` validate and
//! extract, `repository` stores through the narrow `ObjectRepository` trait.

mod adapter;
mod geometry;
mod name;
mod repository;

pub use adapter::{
    GEOJSON_MEDIA_TYPE, PLACE_TYPE, feature_from_place, place_from_feature, place_geo_uri,
};
pub use geometry::feature_bbox;
pub use name::feature_name;
pub use repository::PlaceRepository;
