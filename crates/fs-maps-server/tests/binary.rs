//! The real program: started, used, stopped and started again; the places are still there.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use fs_interfaces::places_capnp::{places, tile_source};
use fs_interfaces::rpc::connect;
use fs_maps_core::testutil::write_fixture;

fn program(socket: &Path, data: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fs-maps-server"));
    command
        .arg("--socket")
        .arg(socket)
        .arg("--data")
        .arg(data)
        .stderr(Stdio::null());
    command
}

async fn wait_for(socket: &Path) {
    for _ in 0..200 {
        if tokio::net::UnixStream::connect(socket).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the server did not come up");
}

fn terminate(mut child: Child) {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(child.wait().unwrap().success());
}

async fn stored_places(socket: &Path) -> usize {
    let connection = connect::<places::Client>(socket).await.unwrap();
    let listed = connection
        .client
        .list_request()
        .send()
        .promise
        .await
        .unwrap();
    let page = listed.get().unwrap().get_page().unwrap();
    page.get_items().unwrap().len() as usize
}

async fn tile_found(socket: &Path, z: u8) -> bool {
    let connection = connect::<tile_source::Client>(socket).await.unwrap();
    let mut req = connection.client.tile_request();
    req.get().set_z(z);
    let answer = req.send().promise.await.unwrap();
    answer.get().unwrap().get_found()
}

#[tokio::test(flavor = "current_thread")]
async fn places_survive_a_restart_and_a_second_start_is_refused() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir.path().join("maps.sock");
            let data = dir.path().join("data");
            let package = dir.path().join("region.pmtiles");
            write_fixture(&package, 2).await.unwrap();
            let start = || {
                program(&socket, &data)
                    .arg("--tiles")
                    .arg(&package)
                    .spawn()
                    .unwrap()
            };

            let first = start();
            wait_for(&socket).await;

            let mut second = start();
            assert!(!second.wait().unwrap().success(), "a second server started");
            assert!(socket.exists(), "the second start touched the first");

            let connection = connect::<places::Client>(&socket).await.unwrap();
            let mut create = connection.client.create_request();
            create.get().set_geojson(
                r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{"name":"Kept"}}"#,
            );
            create.send().promise.await.unwrap();
            drop(connection);
            assert!(tile_found(&socket, 2).await);
            assert!(!tile_found(&socket, 3).await);

            terminate(first);
            assert!(!socket.exists(), "the socket stayed behind");

            let again = start();
            wait_for(&socket).await;
            assert_eq!(stored_places(&socket).await, 1);
            assert!(tile_found(&socket, 2).await);
            terminate(again);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_program_refuses_what_it_cannot_serve() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("maps.sock");
    let data = dir.path().join("data");
    let run = |extra: &[&str]| {
        let status = program(&socket, &data).args(extra).status().unwrap();
        status.code()
    };
    assert_eq!(run(&["--nonsense"]), Some(2));
    assert_eq!(run(&["--cache-mib", "many"]), Some(2));
    assert_eq!(run(&["--tiles", "/nonexistent.pmtiles"]), Some(1));
    let forbidden = "https://tile.openstreetmap.org/x.pmtiles";
    assert_eq!(run(&["--tiles-url", forbidden]), Some(1));
    assert!(!socket.exists(), "a refused start left its socket behind");
    assert_eq!(run(&["--help"]), Some(0));
}
