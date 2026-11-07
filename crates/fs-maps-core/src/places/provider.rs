//! [`StorePlaces`]: the provider a program serves, owning its store and its subscribers.
//!
//! Pattern: Facade + Observer — [`PlaceRepository`] borrows its store, so it cannot live behind an
//! `Arc<dyn Places>`; this one owns an `Arc<dyn Store>`, runs each call through a repository and
//! tells the listeners what was committed. A write that failed tells nobody. Writes run one at a
//! time: an update reads, then writes; a delete between the two would otherwise be undone by it, and
//! the listeners hear the changes in the order they were committed.

use std::sync::Arc;

use async_trait::async_trait;
use fs_interfaces::Capabilities;
use fs_objects::{ObjectId, Store, StoredObject};
use serde_json::Value;
use tokio::sync::Mutex;

use super::book::offered;
use super::bounds::Area;
use super::contract::{Paging, Places};
use super::feed::{PlaceChange, PlaceFeed, PlaceListener, Registry, Subscription};
use super::repository::PlaceRepository;
use super::store_view::StoreView;
use crate::error::Result;

/// Places over a whole store, with the live feed. Clones share their subscribers.
#[derive(Clone)]
pub struct StorePlaces {
    store: Arc<dyn Store>,
    feed: Arc<Registry>,
    writes: Arc<Mutex<()>>,
    actor: String,
    instance: Option<String>,
}

impl StorePlaces {
    /// Places over `store`; `actor` is the IRI recorded in the activity log.
    pub fn new(store: Arc<dyn Store>, actor: impl Into<String>) -> Self {
        Self {
            store,
            feed: Arc::default(),
            writes: Arc::default(),
            actor: actor.into(),
            instance: None,
        }
    }

    /// The instance host used in the IRI `https://<instance>/places/<id>`.
    #[must_use]
    pub fn with_instance(mut self, host: impl Into<String>) -> Self {
        self.instance = Some(host.into());
        self
    }

    fn repository<'a>(&'a self, view: &'a StoreView<'a>) -> PlaceRepository<'a> {
        let repo = PlaceRepository::new(view, self.actor.as_str());
        match &self.instance {
            Some(host) => repo.with_instance(host.as_str()),
            None => repo,
        }
    }

    fn view(&self) -> StoreView<'_> {
        StoreView::new(&*self.store)
    }
}

#[async_trait]
impl Places for StorePlaces {
    fn capabilities(&self) -> Capabilities {
        let mut caps = offered();
        caps.supported("place.subscribe");
        caps
    }
    async fn get(&self, id: ObjectId) -> Result<Option<StoredObject>> {
        let view = self.view();
        Places::get(&self.repository(&view), id).await
    }
    async fn list(&self, page: Paging) -> Result<Vec<StoredObject>> {
        let view = self.view();
        Places::list(&self.repository(&view), page).await
    }
    async fn create(&self, feature: &Value) -> Result<StoredObject> {
        let _one_at_a_time = self.writes.lock().await;
        let view = self.view();
        let place = Places::create(&self.repository(&view), feature).await?;
        self.feed.notify(&PlaceChange::Created(place.envelope.id));
        Ok(place)
    }
    async fn update(&self, id: ObjectId, feature: &Value) -> Result<StoredObject> {
        let _one_at_a_time = self.writes.lock().await;
        let view = self.view();
        let place = Places::update(&self.repository(&view), id, feature).await?;
        self.feed.notify(&PlaceChange::Updated(id));
        Ok(place)
    }
    async fn delete(&self, id: ObjectId) -> Result<()> {
        let _one_at_a_time = self.writes.lock().await;
        let view = self.view();
        Places::delete(&self.repository(&view), id).await?;
        self.feed.notify(&PlaceChange::Deleted(id));
        Ok(())
    }
    async fn within(&self, area: &Area, page: Paging) -> Result<Vec<StoredObject>> {
        let view = self.view();
        Places::within(&self.repository(&view), area, page).await
    }
    async fn find_by_name(&self, term: &str, page: Paging) -> Result<Vec<StoredObject>> {
        let view = self.view();
        Places::find_by_name(&self.repository(&view), term, page).await
    }
}

impl PlaceFeed for StorePlaces {
    fn subscribe(&self, listener: Arc<dyn PlaceListener>) -> Subscription {
        self.feed.add(listener)
    }
}
