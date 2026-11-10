//! The window over a real server: the map screen and one screen per data type, the menus of the shell.
//!
//! Pattern: Integration test (structure only; no picture is drawn, so it needs no graphics).

use std::path::Path;
use std::sync::Arc;

use fs_interfaces::rpc::{bind_socket, serve};
use fs_maps_app::{PlacesApi, PlacesRemote};
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


#[test]
fn the_window_has_the_map_and_a_screen_per_type_and_the_standard_menus() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("maps.sock");
    start_server(&socket);
    let api: Arc<dyn PlacesApi> = Arc::new(PlacesRemote::connect(&socket).unwrap());
    api.create(r#"{"type":"Feature","properties":{"name":"Bern"},"geometry":{"type":"Point","coordinates":[7.447,46.948]}}"#).unwrap();
    let app = fs_maps_app::window::app(&api, 1200.0, 800.0).unwrap();
    app.with_shell(|s| {
        let ids: Vec<&str> = s.screens().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["map", "MapLayer", "MapTour", "Place", "Region"]);
        let entries: Vec<&str> = s.menus().iter().flat_map(|m| m.entries.iter().map(|e| e.id.as_str())).collect();
        assert!(entries.contains(&"view-map") && entries.contains(&"program-quit") && entries.contains(&"help-about"));
    });
    assert!(app.select("Place"));
}
