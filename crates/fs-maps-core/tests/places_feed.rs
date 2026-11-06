//! `StorePlaces`: the owning provider and its live feed, over a real `TursoStore`.

mod common;

use std::sync::{Arc, Mutex};

use common::{ME, point, store};
use fs_maps_core::{
    Area, Error, NoSource, Paging, PlaceChange, PlaceFeed, PlaceListener, Places, StorePlaces,
    TileSource,
};
use fs_store::{ObjectId, Store};

#[derive(Default)]
struct Seen(Mutex<Vec<PlaceChange>>);

impl PlaceListener for Seen {
    fn changed(&self, change: &PlaceChange) {
        self.0.lock().unwrap().push(*change);
    }
}

async fn provider() -> StorePlaces {
    let s: Arc<dyn Store> = Arc::new(store().await);
    StorePlaces::new(s, ME).with_instance("maps.example.org")
}

#[tokio::test]
async fn committed_writes_are_told_until_the_subscription_ends() {
    let places = provider().await;
    let seen = Arc::new(Seen::default());
    let subscription = places.subscribe(seen.clone());

    let id = places.create(&point("Berlin", 13.4, 52.5)).await.unwrap();
    let id = id.envelope.id;
    places
        .update(id, &point("Berlin", 13.5, 52.5))
        .await
        .unwrap();
    places.delete(id).await.unwrap();
    let expected = [
        PlaceChange::Created(id),
        PlaceChange::Updated(id),
        PlaceChange::Deleted(id),
    ];
    assert_eq!(*seen.0.lock().unwrap(), expected);

    drop(subscription);
    places.create(&point("Paris", 2.3, 48.8)).await.unwrap();
    assert_eq!(seen.0.lock().unwrap().len(), 3, "told after the drop");
}

#[tokio::test]
async fn a_failed_write_tells_nobody() {
    let places = provider().await;
    let seen = Arc::new(Seen::default());
    let _subscription = places.subscribe(seen.clone());
    let unknown = ObjectId::new();
    assert!(places.delete(unknown).await.is_err());
    let f = point("x", 0.0, 0.0);
    assert!(places.update(unknown, &f).await.is_err());
    assert!(places.create(&point("x", 999.0, 0.0)).await.is_err());
    assert!(seen.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn it_answers_every_method_and_reports_the_feed() {
    let places = provider().await;
    let p = places.create(&point("Berlin", 13.4, 52.5)).await.unwrap();
    let iri = format!("https://maps.example.org/places/{}", p.envelope.id);
    assert_eq!(p.envelope.iri.as_deref(), Some(iri.as_str()));
    assert!(places.get(p.envelope.id).await.unwrap().is_some());
    assert_eq!(places.list(Paging::default()).await.unwrap().len(), 1);
    let area = Area::new(13.0, 52.0, 14.0, 53.0).unwrap();
    let found = places.within(&area, Paging::default()).await.unwrap();
    assert_eq!(found.len(), 1);
    let named = places
        .find_by_name("berl", Paging::default())
        .await
        .unwrap();
    assert_eq!(named.len(), 1);
    assert!(places.capabilities().is_supported("place.subscribe"));
}

#[tokio::test]
async fn no_source_says_not_possible() {
    let e = NoSource.get_tile(0, 0, 0).await.unwrap_err();
    assert!(matches!(e, Error::Unsupported(_)), "{e:?}");
    assert!(!NoSource.capabilities().is_supported("tile.read"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_update_never_brings_a_deleted_place_back() {
    let places = provider().await;
    for _ in 0..200 {
        let created = places.create(&point("x", 1.0, 1.0)).await.unwrap();
        let id = created.envelope.id;
        let (a, b) = (places.clone(), places.clone());
        let update = tokio::spawn(async move { a.update(id, &point("y", 2.0, 2.0)).await });
        let delete = tokio::spawn(async move { b.delete(id).await });
        let (updated, deleted) = (update.await.unwrap(), delete.await.unwrap());
        assert!(deleted.is_ok(), "{deleted:?}");
        if let Err(e) = updated {
            assert!(matches!(e, Error::NotFound(_)), "{e:?}");
        }
        assert!(places.get(id).await.unwrap().is_none(), "it came back");
    }
}
