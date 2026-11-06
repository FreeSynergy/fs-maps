//! A borrowed `dyn Store` presented as the two narrow traits the place repository needs.
//!
//! Pattern: Adapter — applications hold the `Store` facade (`Arc<dyn Store>`); the repository sees
//! `ObjectRepository` + `ObjectQuery` only: `PlaceRepository::new(&StoreView::new(&*store), actor)`.

use async_trait::async_trait;
use fs_store::traits::{ObjectQuery, ObjectRepository};
use fs_store::{Activity, LogEntry, ObjectId, Query, Result, Store, StoredObject};

/// Borrowed store facade.
pub struct StoreView<'a>(&'a dyn Store);

impl<'a> StoreView<'a> {
    /// View `store` as the narrow traits.
    pub fn new(store: &'a dyn Store) -> Self {
        Self(store)
    }
}

#[async_trait]
impl ObjectRepository for StoreView<'_> {
    async fn put(&self, object: StoredObject, actor: &str) -> Result<Activity> {
        self.0.put(object, actor).await
    }
    async fn put_and_log(
        &self,
        object: StoredObject,
        actor: &str,
        extra: Vec<LogEntry>,
    ) -> Result<Vec<Activity>> {
        self.0.put_and_log(object, actor, extra).await
    }
    async fn get(&self, id: ObjectId) -> Result<Option<StoredObject>> {
        self.0.get(id).await
    }
    async fn delete(&self, id: ObjectId, actor: &str) -> Result<Activity> {
        self.0.delete(id, actor).await
    }
}

#[async_trait]
impl ObjectQuery for StoreView<'_> {
    async fn query(&self, query: &Query) -> Result<Vec<StoredObject>> {
        self.0.query(query).await
    }
    async fn count(&self, query: &Query) -> Result<usize> {
        self.0.count(query).await
    }
}
