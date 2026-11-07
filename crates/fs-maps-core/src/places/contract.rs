//! The [`Places`] contract: what the maps program offers to everyone else.
//!
//! Pattern: Repository (interface) + Null Object — callers depend on this trait, never on the
//! store-backed `PlaceRepository`. It is the Rust form of `Places` in `places.capnp`
//! (fs-interfaces). Like a parent class (ADR 0045) every provider has every method: one it does
//! not override answers an explicit [`Unsupported`], and `capabilities` reports the same.

use async_trait::async_trait;
use fs_interfaces::{Capabilities, Unsupported};
use fs_objects::{ObjectId, StoredObject};
use serde_json::Value;

use super::bounds::Area;
use crate::error::Result;

/// The explicit "not possible" of a method a provider does not override.
pub(crate) fn not_possible<T>(capability: &str) -> Result<T> {
    Err(Unsupported::new(capability, "this provider does not offer it").into())
}

/// One page of a list: the Rust form of the contract's cursor (an offset behind it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Paging {
    /// Maximum number of places; `None` = all.
    pub limit: Option<usize>,
    /// Places to skip.
    pub offset: usize,
}

impl Paging {
    /// A page of `limit` places after skipping `offset`.
    pub fn new(limit: usize, offset: usize) -> Self {
        Self {
            limit: Some(limit),
            offset,
        }
    }
}

/// Places: GeoJSON features with a name and a position, found by area or by name.
///
/// Interface status: draft (ADR 0045).
#[async_trait]
pub trait Places: Send + Sync {
    /// What this provider can do, per capability id.
    fn capabilities(&self) -> Capabilities;
    /// `place.read`: the stored place, if it exists.
    async fn get(&self, id: ObjectId) -> Result<Option<StoredObject>>;
    /// `place.read`: places in name order, paged.
    async fn list(&self, page: Paging) -> Result<Vec<StoredObject>>;

    /// `place.create`: validate and store a GeoJSON `Feature`.
    async fn create(&self, _feature: &Value) -> Result<StoredObject> {
        not_possible("place.create")
    }
    /// `place.update`: replace the feature of `id` (envelope, `published` and id stay).
    async fn update(&self, _id: ObjectId, _feature: &Value) -> Result<StoredObject> {
        not_possible("place.update")
    }
    /// `place.delete`: delete a place (undoable through the store).
    async fn delete(&self, _id: ObjectId) -> Result<()> {
        not_possible("place.delete")
    }
    /// `place.find`: places whose geometry's bounding box overlaps `area` (edges touching count;
    /// an area with `west > east` crosses the antimeridian). Places without geometry never match.
    async fn within(&self, _area: &Area, _page: Paging) -> Result<Vec<StoredObject>> {
        not_possible("place.find")
    }
    /// `place.find`: places whose name (or any `name:*` / `title`) contains `term`, ignoring
    /// case.
    async fn find_by_name(&self, _term: &str, _page: Paging) -> Result<Vec<StoredObject>> {
        not_possible("place.find")
    }
}
