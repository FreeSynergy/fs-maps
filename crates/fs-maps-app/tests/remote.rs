//! The window's adapter over the socket: `PlacesRemote` against a real server and a real store.
//!
//! Pattern: Integration test — the server runs on its own thread and socket, the adapter is the one
//! the window uses.
#![allow(clippy::assert_is_empty)] // tests: plain `is_empty()` asserts read clearer than typed empty comparisons

use std::path::Path;
use std::sync::Arc;

use fs_components_workbenches::ErrorKind;
use fs_components_workbenches::people::{PeopleSource, PersonDraft};
use fs_interfaces::rpc::{bind_socket, serve};
use fs_maps_app::{PlacesAdapter, PlacesApi, PlacesRemote};
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

fn set(draft: &mut PersonDraft, key: &str, value: &str) {
    draft
        .groups
        .iter_mut()
        .flat_map(|g| g.fields.iter_mut())
        .find(|f| f.key == key)
        .unwrap()
        .value = value.into();
}

fn shown(source: &PlacesAdapter, key: &str) -> String {
    source
        .read(key)
        .unwrap()
        .sections
        .iter()
        .flat_map(|s| s.fields.iter().map(|f| format!("{}={}", f.label, f.value)))
        .collect::<Vec<_>>()
        .join(";")
}

#[test]
fn create_list_read_move_describe_delete_over_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("maps.sock");
    start_server(&socket);
    let remote = Arc::new(PlacesRemote::connect(&socket).unwrap());
    let adapter = PlacesAdapter::new(remote.clone());

    let mut draft = adapter.blank().unwrap();
    set(&mut draft, "name", "Brandenburg Gate");
    set(&mut draft, "latitude", "52,5163");
    set(&mut draft, "longitude", "13.3777");
    let key = adapter.create(&draft).unwrap();

    let rows = adapter.rows("", 0, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells, ["Brandenburg Gate", "52.51630, 13.37770"]);
    assert_eq!(adapter.rows("gate", 0, 10).unwrap().len(), 1);
    assert!(adapter.rows("zzz", 0, 10).unwrap().is_empty());
    let sheet = shown(&adapter, &key);
    assert!(sheet.contains("Geo URI=geo:52.5163,13.3777"), "{sheet}");

    let mut edit = adapter.draft(&key).unwrap();
    set(&mut edit, "latitude", "52.5");
    set(&mut edit, "description", "A gate");
    adapter.save(&key, &edit).unwrap();
    let sheet = shown(&adapter, &key);
    assert!(
        sheet.contains("Latitude=52.5") && sheet.contains("Description=A gate"),
        "{sheet}"
    );

    adapter.delete(&key).unwrap();
    assert!(adapter.rows("", 0, 10).unwrap().is_empty());
    assert_eq!(adapter.read(&key).unwrap_err().kind, ErrorKind::NotFound);
}

#[test]
fn a_bad_position_or_a_missing_name_is_refused_with_a_sentence_before_the_server() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("maps.sock");
    start_server(&socket);
    let adapter = PlacesAdapter::new(Arc::new(PlacesRemote::connect(&socket).unwrap()));
    let mut draft = adapter.blank().unwrap();
    assert!(adapter.create(&draft).unwrap_err().message.contains("name"));
    set(&mut draft, "name", "Nowhere");
    set(&mut draft, "latitude", "91");
    set(&mut draft, "longitude", "0");
    assert!(
        adapter
            .create(&draft)
            .unwrap_err()
            .message
            .contains("latitude")
    );
    set(&mut draft, "latitude", "10");
    set(&mut draft, "longitude", "abc");
    assert!(
        adapter
            .create(&draft)
            .unwrap_err()
            .message
            .contains("longitude")
    );
    assert!(adapter.rows("", 0, 10).unwrap().is_empty());
}

#[test]
fn nothing_on_the_socket_is_said_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let message = PlacesRemote::connect(&dir.path().join("none.sock"))
        .err()
        .unwrap();
    assert!(message.contains("fs-maps serve"), "{message}");
}

#[test]
fn a_stale_save_is_a_conflict_and_what_the_form_never_showed_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("maps.sock");
    start_server(&socket);
    let remote = Arc::new(PlacesRemote::connect(&socket).unwrap());
    let adapter = PlacesAdapter::new(remote.clone());
    let made = remote
        .create(
            r#"{"type":"Feature","bbox":[13,52,14,53],"geometry":{"type":"Point","coordinates":[13.4,52.5,34.0]},
               "properties":{"name":"Spot","description":{"en":"a spot","de":"ein Ort"}}}"#,
        )
        .unwrap();

    // Two windows open the same place; the second saves first.
    let mut first = adapter.draft(&made.id).unwrap();
    let mut second = adapter.draft(&made.id).unwrap();
    set(&mut second, "name", "Spot B");
    adapter.save(&made.id, &second).unwrap();
    set(&mut first, "name", "Spot A");
    let refused = adapter.save(&made.id, &first).unwrap_err();
    assert_eq!(refused.kind, ErrorKind::Conflict, "{refused:?}");

    // A rename leaves a description that is no plain text, the altitude and the box alone.
    let kept: serde_json::Value =
        serde_json::from_str(&remote.get(&made.id).unwrap().unwrap().geojson).unwrap();
    assert_eq!(kept["properties"]["name"], "Spot B");
    assert_eq!(kept["properties"]["description"]["de"], "ein Ort");
    assert_eq!(kept["geometry"]["coordinates"][2], 34.0);
    assert!(kept.get("bbox").is_some(), "{kept}");

    // Moving the point drops the box (it would not contain it any more), the altitude stays.
    let mut third = adapter.draft(&made.id).unwrap();
    set(&mut third, "latitude", "10");
    adapter.save(&made.id, &third).unwrap();
    let moved: serde_json::Value =
        serde_json::from_str(&remote.get(&made.id).unwrap().unwrap().geojson).unwrap();
    assert_eq!(
        moved["geometry"]["coordinates"],
        serde_json::json!([13.4, 10.0, 34.0])
    );
    assert!(moved.get("bbox").is_none(), "{moved}");
}
