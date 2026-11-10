//! The records and commands of the window over the socket: `PlacesBackend` and `PlacesRemote` against a real server
//! and a real store.
//!
//! Pattern: Integration test — the server runs on its own thread and socket, the adapter is the one
//! the window uses.
#![allow(clippy::assert_is_empty)] // tests: plain `is_empty()` asserts read clearer than typed empty comparisons

use std::path::Path;
use std::sync::Arc;

use fs_admin_frontend::{Backend, BackendError, Values};
use fs_interfaces::rpc::{bind_socket, serve};
use fs_maps_app::{PlacesApi, PlacesBackend, PlacesRemote};
use fs_maps_core::StorePlaces;
use fs_maps_server::MapsServer;
use fs_objects::{Store, TursoStore};

const ME: &str = "https://local.invalid/users/owner";

fn start_server(socket: &Path) {
    let socket = socket.to_path_buf();
    let (up, started) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        tokio::task::LocalSet::new().block_on(&runtime, async move {
            let store: Arc<dyn Store> = Arc::new(TursoStore::open_in_memory().await.unwrap());
            let places = Arc::new(StorePlaces::new(store, ME));
            let server = MapsServer::new(places.clone(), places, Arc::new(fs_maps_core::NoSource));
            let bound = bind_socket(&socket).await.unwrap();
            up.send(()).unwrap();
            serve(bound, server.into_client().client, std::future::pending())
                .await
                .ok();
        });
    });
    started.recv().unwrap();
}

fn v(pairs: &[(&str, &[&str])]) -> Values {
    pairs
        .iter()
        .map(|(k, vs)| ((*k).to_owned(), vs.iter().map(|s| (*s).to_owned()).collect()))
        .collect()
}

fn connect() -> (tempfile::TempDir, Arc<PlacesRemote>) {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("maps.sock");
    start_server(&socket);
    let remote = Arc::new(PlacesRemote::connect(&socket).unwrap());
    (dir, remote)
}

#[test]
fn the_description_comes_over_the_socket_with_every_type() {
    let (_dir, remote) = connect();
    let types = remote.types().unwrap();
    let names: Vec<_> = types.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["MapLayer", "MapTour", "Place", "Region"]);
}

#[test]
fn create_find_within_change_delete_over_the_socket() {
    let (_dir, remote) = connect();
    let backend = PlacesBackend::new(remote.clone(), "Place");
    let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t = told.clone();
    backend.subscribe(Arc::new(move || {
        t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let made = backend
        .run("create", &v(&[("name", &["Bern"]), ("position", &["geo:46.948,7.447"]), ("keywords", &["capital", "swiss"])]))
        .unwrap();
    let id = made.id.unwrap();
    backend.run("create", &v(&[("name", &["Wien"]), ("position", &["geo:48.2,16.37"])])).unwrap();
    let all = backend.list(&Values::new()).unwrap();
    assert_eq!(all.len(), 2);
    let bern = all.iter().find(|r| r.id == id).unwrap();
    assert_eq!(bern.values["name"], ["Bern"]);
    assert_eq!(bern.values["keywords"], ["capital", "swiss"]);
    assert!(bern.values["position"][0].starts_with("geo:46.948"));
    // find by name
    assert_eq!(backend.list(&v(&[("term", &["wie"])])).unwrap().len(), 1);
    // within a box around Bern only
    let boxed = v(&[("southwest", &["geo:46,7"]), ("northeast", &["geo:47,8"])]);
    let inside = backend.list(&boxed).unwrap();
    assert_eq!(inside.len(), 1);
    assert_eq!(inside[0].id, id);
    // change is partial: the description is added, the name and the keywords stay
    backend.run("change", &v(&[("id", &[id.as_str()]), ("description", &["on the Aare"])])).unwrap();
    let r = backend.list(&v(&[("term", &["bern"])])).unwrap().remove(0);
    assert_eq!(r.values["description"], ["on the Aare"]);
    assert_eq!(r.values["name"], ["Bern"]);
    assert_eq!(r.values["keywords"], ["capital", "swiss"]);
    // an empty list clears, a new value replaces
    backend.run("change", &v(&[("id", &[id.as_str()]), ("keywords", &[]), ("position", &["geo:46.9,7.4"])])).unwrap();
    let r = backend.list(&v(&[("term", &["bern"])])).unwrap().remove(0);
    assert!(!r.values.contains_key("keywords"));
    assert!(r.values["position"][0].starts_with("geo:46.9,7.4"));
    backend.run("delete", &v(&[("id", &[id.as_str()])])).unwrap();
    assert_eq!(backend.list(&Values::new()).unwrap().len(), 1);
    assert_eq!(told.load(std::sync::atomic::Ordering::SeqCst), 5, "every write told its listener");
}

#[test]
fn a_bad_position_or_a_missing_name_is_rejected_with_the_field() {
    let (_dir, remote) = connect();
    let backend = PlacesBackend::new(remote, "Place");
    for input in [v(&[("name", &["x"]), ("position", &["not a place"])]), v(&[("name", &[" "])])] {
        match backend.run("create", &input) {
            Err(BackendError::Rejected(issues)) => assert_eq!(issues.len(), 1),
            other => panic!("{other:?}"),
        }
    }
    assert!(backend.list(&Values::new()).unwrap().is_empty());
}

#[test]
fn commands_the_core_does_not_have_are_answered_unsupported_and_send_nothing() {
    let (_dir, remote) = connect();
    for (ty, cmd) in [("Place", "elevationProfile"), ("MapLayer", "create"), ("MapTour", "delete"), ("Region", "download")] {
        let backend = PlacesBackend::new(remote.clone(), ty);
        assert_eq!(
            backend.run(cmd, &Values::new()),
            Err(BackendError::Unavailable(format!("unsupported:{cmd}")))
        );
        assert!(backend.list(&Values::new()).unwrap().is_empty());
    }
    let places = PlacesBackend::new(remote.clone(), "Place");
    assert!(places.list(&Values::new()).unwrap().is_empty(), "nothing was created");
}

#[test]
fn a_shape_keeps_its_geometry_and_refuses_a_position() {
    let (_dir, remote) = connect();
    let line = r#"{"type":"Feature","properties":{"name":"Road","foreign":1},"geometry":{"type":"LineString","coordinates":[[7,46],[8,47]]}}"#;
    let id = remote.create(line).unwrap().id;
    let backend = PlacesBackend::new(remote.clone(), "Place");
    let rec = backend.list(&Values::new()).unwrap().remove(0);
    assert!(!rec.values.contains_key("position"));
    assert!(matches!(
        backend.run("change", &v(&[("id", &[id.as_str()]), ("position", &["geo:1,2"])])),
        Err(BackendError::Rejected(_))
    ));
    backend.run("change", &v(&[("id", &[id.as_str()]), ("name", &["Road 2"])])).unwrap();
    let kept = remote.get(&id).unwrap().unwrap().geojson;
    assert!(kept.contains("LineString") && kept.contains("foreign"), "{kept}");
}

#[test]
fn nothing_on_the_socket_is_said_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let e = PlacesRemote::connect(&dir.path().join("none.sock")).err().unwrap();
    assert!(e.contains("no maps server"), "{e}");
}

#[test]
fn tiles_come_over_the_socket_as_not_available_when_no_map_is_configured() {
    let (_dir, remote) = connect();
    assert!(remote.tile(0, 0, 0).is_err());
}
