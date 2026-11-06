//! Create, update and delete of places.
//!
//! Pattern: Template method — one flow: build the object from the feature (validation and index
//! in the adapter), complete the envelope (context, IRI), write. An update keeps the stored
//! envelope (id, `published`, audience) and takes only name and content from the new feature.

use fs_store::{ObjectId, StoredObject};
use serde_json::Value;

use super::adapter::{PLACE_TYPE, place_from_feature};
use super::repository::{CONTEXT, PlaceRepository};
use crate::error::{Error, Result};

impl PlaceRepository<'_> {
    /// Validate and store a feature; returns the new object id.
    pub async fn save(&self, feature: &Value) -> Result<ObjectId> {
        Ok(self.create_object(feature).await?.envelope.id)
    }

    pub(super) async fn create_object(&self, feature: &Value) -> Result<StoredObject> {
        let mut obj = place_from_feature(feature, Some(&self.actor))?;
        obj.envelope.context = Some(CONTEXT.into());
        obj.envelope.iri = Some(format!(
            "https://{}/{CONTEXT}/{}",
            self.instance, obj.envelope.id
        ));
        let id = obj.envelope.id;
        self.repo.put(obj, &self.actor).await?;
        self.stored(id).await
    }

    pub(super) async fn update_object(
        &self,
        id: ObjectId,
        feature: &Value,
    ) -> Result<StoredObject> {
        let old = self.stored(id).await?;
        let mut obj = place_from_feature(feature, old.envelope.attributed_to.as_deref())?;
        let mut envelope = old.envelope;
        envelope.name = obj.envelope.name;
        obj.envelope = envelope;
        self.repo.put(obj, &self.actor).await?;
        self.stored(id).await
    }

    pub(super) async fn delete_object(&self, id: ObjectId) -> Result<()> {
        self.stored(id).await?;
        self.repo.delete(id, &self.actor).await?;
        Ok(())
    }

    /// The object `id`, which must be a `Place`.
    pub(super) async fn stored(&self, id: ObjectId) -> Result<StoredObject> {
        match self.repo.get(id).await? {
            Some(o) if o.envelope.object_type == PLACE_TYPE => Ok(o),
            _ => Err(Error::NotFound(id)),
        }
    }
}
