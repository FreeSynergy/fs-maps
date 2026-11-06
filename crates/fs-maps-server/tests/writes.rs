//! A write finishes, and is told, whatever the client does meanwhile.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::seen::{Seen, Told, watch};
use common::{point, serving, store_places};
use fs_interfaces::Capabilities;
use fs_maps_core::{NoSource, Paging, Places, Result, StorePlaces};
use fs_store::{ObjectId, StoredObject};
use serde_json::Value;

/// The store-backed provider, a moment slower: time for the client to go away.
struct Slow(Arc<StorePlaces>);

#[async_trait]
impl Places for Slow {
    fn capabilities(&self) -> Capabilities {
        self.0.capabilities()
    }
    async fn get(&self, id: ObjectId) -> Result<Option<StoredObject>> {
        self.0.get(id).await
    }
    async fn list(&self, page: Paging) -> Result<Vec<StoredObject>> {
        self.0.list(page).await
    }
    async fn create(&self, feature: &Value) -> Result<StoredObject> {
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.0.create(feature).await
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_client_that_drops_its_call_does_not_stop_the_write() {
    let places = store_places().await;
    let provider = Arc::new(Slow(places.clone()));
    let stored = places.clone();
    serving(provider, places, Arc::new(NoSource), |maps| async move {
        let p = maps.places;
        let seen = Seen::default();
        let _answer = watch(&p, &seen).await;
        let mut create = p.create_request();
        create.get().set_geojson(point("Kept", 1.0, 1.0).as_str());
        let call = create.send().promise;
        // On the wire, then given up: the server is told to cancel it.
        p.support_report_request().send().promise.await.unwrap();
        drop(call);
        seen.until(1).await;
        let all = stored.list(Paging::default()).await.unwrap();
        assert_eq!(all.len(), 1);
        let id = all[0].envelope.id;
        assert_eq!(seen.told(), [Told::Created(id, "Kept".into())]);
    })
    .await;
}
