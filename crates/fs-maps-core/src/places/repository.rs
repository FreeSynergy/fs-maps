//! Places stored through the narrow object-repository interface.
//!
//! Pattern: Repository over Store — depends on `ObjectRepository` only (not the whole `Store`
//! facade), so any backend or test double with put/get fits.

use fs_store::ObjectId;
use fs_store::traits::ObjectRepository;
use serde_json::Value;

use super::adapter::{feature_from_place, place_from_feature};
use crate::error::Result;

/// Save and load GeoJSON features as `Place` objects.
pub struct PlaceRepository<'a> {
    repo: &'a dyn ObjectRepository,
    actor: String,
}

impl<'a> PlaceRepository<'a> {
    /// Repository acting as `actor` (an IRI, recorded in the activity log).
    pub fn new(repo: &'a dyn ObjectRepository, actor: impl Into<String>) -> Self {
        Self {
            repo,
            actor: actor.into(),
        }
    }

    /// Validate and store a feature; returns the new object id.
    pub async fn save(&self, feature: &Value) -> Result<ObjectId> {
        let obj = place_from_feature(feature, Some(&self.actor))?;
        let id = obj.envelope.id;
        self.repo.put(obj, &self.actor).await?;
        Ok(id)
    }

    /// Load a place back as a GeoJSON feature.
    pub async fn load(&self, id: ObjectId) -> Result<Option<Value>> {
        self.repo
            .get(id)
            .await?
            .map(|o| feature_from_place(&o))
            .transpose()
    }
}
