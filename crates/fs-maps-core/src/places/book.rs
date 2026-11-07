//! `Places` implemented by `PlaceRepository`.
//!
//! Pattern: Facade — each trait method forwards to the flow behind it (`create_object`,
//! `places_within`, ...), so the contract stays one thin file.

use async_trait::async_trait;
use fs_interfaces::Capabilities;
use fs_objects::{ObjectId, StoredObject};
use serde_json::Value;

use super::bounds::Area;
use super::contract::{Paging, Places};
use super::repository::PlaceRepository;
use crate::error::Result;

#[async_trait]
impl Places for PlaceRepository<'_> {
    fn capabilities(&self) -> Capabilities {
        capabilities()
    }
    async fn get(&self, id: ObjectId) -> Result<Option<StoredObject>> {
        match self.stored(id).await {
            Ok(o) => Ok(Some(o)),
            Err(crate::error::Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }
    async fn list(&self, page: Paging) -> Result<Vec<StoredObject>> {
        self.list_places(page).await
    }
    async fn create(&self, feature: &Value) -> Result<StoredObject> {
        self.create_object(feature).await
    }
    async fn update(&self, id: ObjectId, feature: &Value) -> Result<StoredObject> {
        self.update_object(id, feature).await
    }
    async fn delete(&self, id: ObjectId) -> Result<()> {
        self.delete_object(id).await
    }
    async fn within(&self, area: &Area, page: Paging) -> Result<Vec<StoredObject>> {
        self.places_within(area, page).await
    }
    async fn find_by_name(&self, term: &str, page: Paging) -> Result<Vec<StoredObject>> {
        self.places_named(term, page).await
    }
}

/// The store-backed provider offers every method, except the live feed, which is not built yet.
pub(super) fn capabilities() -> Capabilities {
    let mut caps = Capabilities::new("fs-maps");
    for id in [
        "place.create",
        "place.update",
        "place.delete",
        "place.read",
        "place.find",
    ] {
        caps.supported(id);
    }
    caps.unsupported("place.subscribe", "not built yet; it is possible");
    caps
}
