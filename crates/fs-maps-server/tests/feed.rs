//! `subscribe` over a real socket: `Created`, `Updated`, `Deleted`, and how a feed ends.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::seen::{Seen, Told, settle, watch};
use common::{create, delete, point, serving, store_places, with_server};
use fs_interfaces::Capabilities;
use fs_interfaces::rpc::{FEED_ENDED_PROBLEM_TYPE, write_object_id};
use fs_maps_core::{Error, NoSource, Paging, Places, Result, StorePlaces};
use fs_store::{ObjectId, StoredObject};
use serde_json::Value;

#[tokio::test(flavor = "current_thread")]
async fn subscribers_see_changes_until_they_drop_the_capability() {
    with_server(|maps| async move {
        let p = maps.places;
        let (seen, other) = (Seen::default(), Seen::default());
        let answer = watch(&p, &seen).await;
        let subscription = answer.get().unwrap().get_subscription().unwrap();
        let kept = watch(&p, &other).await;

        let id = create(&p, "Watched", 13.4, 52.5).await;
        settle().await; // the envelope of a change is read when it goes out
        let mut update = p.update_request();
        write_object_id(update.get().init_id(), id);
        update
            .get()
            .set_geojson(point("Moved", 13.5, 52.5).as_str());
        update.send().promise.await.unwrap();
        settle().await;
        delete(&p, id).await;
        settle().await;
        let expected = [
            Told::Created(id, "Watched".into()),
            Told::Updated(id, "Moved".into()),
            Told::Deleted(id),
        ];
        assert_eq!(seen.told(), expected);
        assert_eq!(other.told(), expected);

        drop(subscription);
        drop(answer);
        settle().await;
        let later = create(&p, "Unwatched", 2.3, 48.8).await;
        settle().await;
        assert_eq!(seen.told(), expected, "told after the drop, or told ended");
        assert_eq!(
            other.told().last(),
            Some(&Told::Created(later, "Unwatched".into()))
        );
        drop(kept);
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_write_is_not_a_change() {
    with_server(|maps| async move {
        let p = maps.places;
        let seen = Seen::default();
        let _answer = watch(&p, &seen).await;
        let mut bad = p.create_request();
        bad.get().set_geojson("{ not json");
        assert!(bad.send().promise.await.is_err());
        let mut gone = p.delete_request();
        write_object_id(gone.get().init_id(), ObjectId::new());
        assert!(gone.send().promise.await.is_err());
        settle().await;
        assert_eq!(seen.told(), []);
    })
    .await;
}

/// The store-backed provider, whose reads answer as told; everything else goes through.
struct Reads(Arc<StorePlaces>, fn() -> Result<Option<StoredObject>>);

#[async_trait]
impl Places for Reads {
    fn capabilities(&self) -> Capabilities {
        self.0.capabilities()
    }
    async fn get(&self, _: ObjectId) -> Result<Option<StoredObject>> {
        (self.1)()
    }
    async fn list(&self, page: Paging) -> Result<Vec<StoredObject>> {
        self.0.list(page).await
    }
    async fn create(&self, feature: &Value) -> Result<StoredObject> {
        self.0.create(feature).await
    }
    async fn delete(&self, id: ObjectId) -> Result<()> {
        self.0.delete(id).await
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_that_fails_ends_the_feed_and_says_so() {
    let places = store_places().await;
    let failing = || Err(Error::Store(fs_store::Error::Backend("disk gone".into())));
    let provider = Arc::new(Reads(places.clone(), failing));
    serving(provider, places, Arc::new(NoSource), |maps| async move {
        let p = maps.places;
        let seen = Seen::default();
        let _answer = watch(&p, &seen).await;
        let id = create(&p, "Not told", 1.0, 1.0).await;
        settle().await;
        assert_eq!(
            seen.told(),
            [Told::Ended(FEED_ENDED_PROBLEM_TYPE.into(), 410)]
        );
        delete(&p, id).await;
        settle().await;
        assert_eq!(seen.told().len(), 1, "told after the end");
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_place_gone_by_now_is_skipped_and_the_feed_goes_on() {
    let places = store_places().await;
    let provider = Arc::new(Reads(places.clone(), || Ok(None)));
    serving(provider, places, Arc::new(NoSource), |maps| async move {
        let p = maps.places;
        let seen = Seen::default();
        let _answer = watch(&p, &seen).await;
        let id = create(&p, "Gone at once", 1.0, 1.0).await;
        delete(&p, id).await;
        settle().await;
        assert_eq!(seen.told(), [Told::Deleted(id)]);
    })
    .await;
}
