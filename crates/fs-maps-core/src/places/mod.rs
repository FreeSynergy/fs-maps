//! Places: GeoJSON (RFC 7946) `Feature` <-> `fs-objects` object of AS2 type `Place`.
//!
//! Pattern: Adapter (+ Repository on top) — `adapter` converts, `geometry`/`name` validate and
//! extract, `bounds` turns boxes into store index keys and conditions, `repository` / `write` /
//! `search` store and find through the narrow `ObjectRepository` and `ObjectQuery` traits, and
//! `contract` is the [`Places`] trait callers depend on.

mod adapter;
mod book;
mod bounds;
mod contract;
mod geometry;
mod name;
mod repository;
mod search;
mod store_view;
mod write;

pub use adapter::{
    GEOJSON_MEDIA_TYPE, PLACE_TYPE, feature_from_place, place_from_feature, place_geo_uri,
};
pub use bounds::Area;
pub use contract::{Paging, Places};
pub use geometry::feature_bbox;
pub use name::{feature_name, feature_names};
pub use repository::{CONTEXT, PlaceRepository};
pub use store_view::StoreView;
