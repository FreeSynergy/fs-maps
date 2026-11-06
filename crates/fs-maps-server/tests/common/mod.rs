//! Shared helpers of the integration tests: a real server on a real Unix socket, over a real store.

#![allow(dead_code)] // each test file uses a different subset

pub mod seen;

use std::future::Future;
use std::sync::Arc;

use fs_interfaces::places_capnp::{place, place_page, places, tile_source};
use fs_interfaces::rpc::{Problem, bind_socket, connect, read_envelope, serve, write_object_id};
use fs_maps_core::{NoSource, PlaceFeed, Places, StorePlaces, TileSource};
use fs_maps_server::MapsServer;
use fs_store::{Envelope, ObjectId, Store, TursoStore};

pub const ME: &str = "https://local.invalid/users/owner";

pub fn point(name: &str, lon: f64, lat: f64) -> String {
    format!(
        r#"{{"type":"Feature","geometry":{{"type":"Point","coordinates":[{lon},{lat}]}},"properties":{{"name":"{name}"}}}}"#
    )
}

pub async fn store_places() -> Arc<StorePlaces> {
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    Arc::new(StorePlaces::new(store, ME))
}

/// Both interfaces, connected to the one socket as a consumer would.
#[derive(Clone)]
pub struct Consumer {
    pub places: places::Client,
    pub tiles: tile_source::Client,
}

/// Serve the given provider and source on a socket for the length of `test`.
pub async fn serving<F, Fut>(
    provider: Arc<dyn Places>,
    feed: Arc<dyn PlaceFeed>,
    tiles: Arc<dyn TileSource>,
    test: F,
) where
    F: FnOnce(Consumer) -> Fut,
    Fut: Future<Output = ()>,
{
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("maps.sock");
            let root = MapsServer::new(provider, feed, tiles).into_client();
            let bound = bind_socket(&path).await.unwrap();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let serving = tokio::task::spawn_local(serve(bound, root, async {
                let _ = stopped.await;
            }));
            let first = connect::<places::Client>(&path).await.unwrap();
            let second = connect::<tile_source::Client>(&path).await.unwrap();
            test(Consumer {
                places: first.client.clone(),
                tiles: second.client.clone(),
            })
            .await;
            drop((first, second));
            let _ = stop.send(());
            serving.await.unwrap().unwrap();
        })
        .await;
}

/// The store-backed provider, with the given source of tiles.
pub async fn with_tiles<F, Fut>(tiles: Arc<dyn TileSource>, test: F)
where
    F: FnOnce(Consumer) -> Fut,
    Fut: Future<Output = ()>,
{
    let provider = store_places().await;
    serving(provider.clone(), provider, tiles, test).await;
}

/// The store-backed provider, no tiles, and the store itself for what another writer could leave.
pub async fn with_store<F, Fut>(test: F)
where
    F: FnOnce(Consumer, Arc<dyn Store>) -> Fut,
    Fut: Future<Output = ()>,
{
    let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
    let provider = Arc::new(StorePlaces::new(store.clone(), ME));
    serving(provider.clone(), provider, Arc::new(NoSource), |maps| {
        test(maps, store)
    })
    .await;
}

/// The store-backed provider, no tiles configured.
pub async fn with_server<F, Fut>(test: F)
where
    F: FnOnce(Consumer) -> Fut,
    Fut: Future<Output = ()>,
{
    with_tiles(Arc::new(NoSource), test).await;
}

pub fn envelope_of(place: place::Reader<'_>) -> Envelope {
    read_envelope(place.get_envelope().unwrap()).unwrap()
}

/// Create a place from GeoJSON text; answers its id.
pub async fn create_text(places: &places::Client, geojson: &str) -> ObjectId {
    let mut req = places.create_request();
    req.get().set_geojson(geojson);
    let answer = req.send().promise.await.unwrap();
    envelope_of(answer.get().unwrap().get_place().unwrap()).id
}

pub async fn create(places: &places::Client, name: &str, lon: f64, lat: f64) -> ObjectId {
    create_text(places, &point(name, lon, lat)).await
}

pub async fn delete(places: &places::Client, id: ObjectId) {
    let mut req = places.delete_request();
    write_object_id(req.get().init_id(), id);
    req.send().promise.await.unwrap();
}

/// The problem an error carries; a failure without one fails the test.
pub fn problem(e: &capnp::Error) -> Problem {
    Problem::from_error(e).unwrap_or_else(|| panic!("untyped error: {e}"))
}

/// Type and status of a failed call; an answer instead of a failure fails the test.
pub fn refused<T>(answer: capnp::Result<T>) -> (String, u16) {
    let e = answer.err().expect("expected a refusal");
    let p = problem(&e);
    (p.problem_type, p.status)
}

pub fn is(kind: &str, status: u16) -> (String, u16) {
    (kind.to_string(), status)
}

pub const INVALID: &str = "https://freesynergy.net/problems/places/invalid";
pub const NOT_FOUND: &str = "https://freesynergy.net/problems/places/not-found";
pub const STORED_DATA: &str = "https://freesynergy.net/problems/places/stored-data-invalid";
pub const INVALID_REQUEST: &str = "https://freesynergy.net/problems/invalid-request";
pub const TOO_LARGE: &str = "https://freesynergy.net/problems/too-large";

/// What one page holds, as (id, name), and the next cursor.
pub fn page_of(page: place_page::Reader<'_>) -> (Vec<(ObjectId, String)>, Vec<u8>) {
    let items = page
        .get_items()
        .unwrap()
        .iter()
        .map(|p| {
            let e = envelope_of(p);
            (e.id, e.name)
        })
        .collect();
    let next = page.get_next().unwrap().get_opaque().unwrap().to_vec();
    (items, next)
}

/// Page through `list` to the end: all ids in the order given, and how many pages it took.
pub async fn list_all(places: &places::Client) -> (Vec<ObjectId>, usize) {
    let (mut ids, mut pages, mut cursor) = (Vec::new(), 0, Vec::new());
    loop {
        let mut req = places.list_request();
        req.get().init_cursor().set_opaque(&cursor);
        let answer = req.send().promise.await.unwrap();
        let (items, next) = page_of(answer.get().unwrap().get_page().unwrap());
        ids.extend(items.into_iter().map(|(id, _)| id));
        pages += 1;
        cursor = next;
        if cursor.is_empty() {
            return (ids, pages);
        }
    }
}
