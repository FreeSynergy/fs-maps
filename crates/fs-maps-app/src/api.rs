//! What the window asks of the maps program: the trait, its values and its errors.
//!
//! Pattern: Repository (interface, trait first) — [`PlacesApi`] is what the backend and the map depend on; the socket
//! is behind it (`remote.rs`).

use fs_types::TypeDescription;

/// A place: its id, its name and its `GeoJSON` feature as the server keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The id.
    pub id: String,
    /// The name.
    pub name: String,
    /// The `GeoJSON` `Feature`.
    pub geojson: String,
}

/// Why a call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// There is no such place.
    NotFound(String),
    /// The server cannot do this (the provider rule).
    Unsupported(String),
    /// The server refused or failed; the text is its own.
    Failed(String),
}

/// What the window asks of the maps program.
pub trait PlacesApi: Send + Sync {
    /// All places, in the server's order.
    ///
    /// # Errors
    /// [`ApiError`].
    fn list(&self) -> Result<Vec<Place>, ApiError>;
    /// One place, `None` if there is none.
    ///
    /// # Errors
    /// [`ApiError`].
    fn get(&self, id: &str) -> Result<Option<Place>, ApiError>;
    /// A new place from a `GeoJSON` feature.
    ///
    /// # Errors
    /// [`ApiError`].
    fn create(&self, geojson: &str) -> Result<Place, ApiError>;
    /// Replace a place with a `GeoJSON` feature.
    ///
    /// # Errors
    /// [`ApiError`].
    fn update(&self, id: &str, geojson: &str) -> Result<Place, ApiError>;
    /// Delete a place.
    ///
    /// # Errors
    /// [`ApiError`].
    fn delete(&self, id: &str) -> Result<(), ApiError>;
    /// Places whose name contains `term`, in the server's order.
    ///
    /// # Errors
    /// [`ApiError`].
    fn find_by_name(&self, term: &str) -> Result<Vec<Place>, ApiError>;
    /// Places whose bounding box meets the box `(west, south, east, north)` (`west > east` crosses 180).
    ///
    /// # Errors
    /// [`ApiError`].
    fn within(&self, area: (f64, f64, f64, f64)) -> Result<Vec<Place>, ApiError>;
    /// The data types the program describes (`Places.types`).
    ///
    /// # Errors
    /// [`ApiError`].
    fn types(&self) -> Result<Vec<TypeDescription>, ApiError>;
    /// One vector tile (uncompressed MVT); `None` = there is none.
    ///
    /// # Errors
    /// [`ApiError`].
    fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>, ApiError>;
}
