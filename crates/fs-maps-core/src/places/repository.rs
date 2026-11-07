//! Places stored through the narrow object-repository interfaces.
//!
//! Pattern: Repository over Store — depends on `ObjectRepository` and `ObjectQuery` only (not
//! the whole `Store` facade), so any backend or test double fits. The writes are in `write.rs`,
//! the searches in `search.rs`, the [`Places`](super::Places) trait in `book.rs`.

use fs_objects::ObjectId;
use fs_objects::traits::{ObjectQuery, ObjectRepository};
use serde_json::Value;

use super::adapter::feature_from_place;
use crate::error::Result;

/// AS2 context of stored places; also the first segment of their IRI.
pub const CONTEXT: &str = "places";

/// Create, change, find and read GeoJSON features as `Place` objects.
pub struct PlaceRepository<'a> {
    pub(super) repo: &'a dyn ObjectRepository,
    pub(super) reads: &'a dyn ObjectQuery,
    pub(super) actor: String,
    pub(super) instance: String,
}

impl<'a> PlaceRepository<'a> {
    /// Repository acting as `actor` (an IRI, recorded in the activity log) over a store that can
    /// write and query, e.g. a `TursoStore`. Behind an `Arc<dyn Store>` use
    /// [`StoreView`](super::StoreView).
    pub fn new<S>(store: &'a S, actor: impl Into<String>) -> Self
    where
        S: ObjectRepository + ObjectQuery,
    {
        Self::with_backend(store, store, actor)
    }

    /// Repository over separate write and read halves (a remote or fake backend can supply them).
    pub fn with_backend(
        repo: &'a dyn ObjectRepository,
        reads: &'a dyn ObjectQuery,
        actor: impl Into<String>,
    ) -> Self {
        Self {
            repo,
            reads,
            actor: actor.into(),
            instance: "local.invalid".into(),
        }
    }

    /// The instance host used in the IRI `https://<instance>/places/<id>` (default
    /// `local.invalid`).
    #[must_use]
    pub fn with_instance(mut self, host: impl Into<String>) -> Self {
        self.instance = host.into();
        self
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
