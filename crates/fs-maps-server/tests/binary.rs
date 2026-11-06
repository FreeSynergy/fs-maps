//! The real program: started, used, stopped and started again; its data directory.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::{Problem, connect};
use fs_maps_core::testutil::{RangeServer, write_fixture};

fn program(socket: &Path, data: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fs-maps-server"));
    command
        .arg("--socket")
        .arg(socket)
        .arg("--data")
        .arg(data)
        .stdout(Stdio::null())
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
    assert_eq!(child.wait().unwrap().code(), Some(0));
}

async fn stored_names(socket: &Path) -> Vec<String> {
    let connection = connect::<places::Client>(socket).await.unwrap();
    let listed = connection
        .client
        .list_request()
        .send()
        .promise
        .await
        .unwrap();
    let page = listed.get().unwrap().get_page().unwrap();
    page.get_items()
        .unwrap()
        .iter()
        .map(|p| {
            let envelope = p.get_envelope().unwrap();
            envelope.get_name().unwrap().to_string().unwrap()
        })
        .collect()
}

async fn tile(socket: &Path, z: u8) -> capnp::Result<Option<Vec<u8>>> {
    let connection = connect::<places::Client>(socket).await.unwrap();
    let tiles = connection
        .client
        .tiles_request()
        .send()
        .pipeline
        .get_source();
    let mut req = tiles.tile_request();
    req.get().set_z(z);
    let answer = req.send().promise.await?;
    let r = answer.get()?;
    Ok(r.get_found().then(|| r.get_mvt().unwrap().to_vec()))
}

fn expected_tile(z: u8) -> Vec<u8> {
    format!("MVT-FIXTURE {z}/0/0 {}", "~".repeat(200)).into_bytes()
}

fn paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("maps.sock"), dir.join("data"))
}

#[tokio::test(flavor = "current_thread")]
async fn places_survive_a_restart_and_a_second_start_is_refused() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let (socket, data) = paths(dir.path());
            let package = dir.path().join("region.pmtiles");
            write_fixture(&package, 2).await.unwrap();
            let start = || {
                program(&socket, &data)
                    .env("FS_MAPS_TILES", &package)
                    .spawn()
                    .unwrap()
            };

            let first = start();
            wait_for(&socket).await;
            let mode = std::fs::metadata(&data).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "the data directory is not private");

            let mut second = start();
            assert_eq!(second.wait().unwrap().code(), Some(1), "a second server");
            assert!(socket.exists(), "the second start touched the first");

            let connection = connect::<places::Client>(&socket).await.unwrap();
            let mut create = connection.client.create_request();
            create.get().set_geojson(
                r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{"name":"Kept"}}"#,
            );
            create.send().promise.await.unwrap();
            drop(connection);
            assert_eq!(tile(&socket, 2).await.unwrap().unwrap(), expected_tile(2));
            assert_eq!(tile(&socket, 3).await.unwrap(), None);

            terminate(first);
            assert!(!socket.exists(), "the socket stayed behind");

            let again = start();
            wait_for(&socket).await;
            assert_eq!(stored_names(&socket).await, ["Kept"]);
            assert_eq!(tile(&socket, 2).await.unwrap().unwrap(), expected_tile(2));
            terminate(again);
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn after_the_archive_is_gone_the_cache_still_answers_and_a_restart_works() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let (socket, data) = paths(dir.path());
            let cache = dir.path().join("cache");
            let archive = dir.path().join("remote.pmtiles");
            write_fixture(&archive, 2).await.unwrap();
            let server = RangeServer::start(&archive).await.unwrap();
            let url = server.url.clone();
            let start = || {
                program(&socket, &data)
                    .env("FS_MAPS_TILES_URL", &url)
                    .env("FS_MAPS_CACHE", &cache)
                    .env("FS_MAPS_CACHE_MIB", "1")
                    .spawn()
                    .unwrap()
            };

            let online = start();
            wait_for(&socket).await;
            assert_eq!(tile(&socket, 1).await.unwrap().unwrap(), expected_tile(1));
            terminate(online);
            drop(server);

            let offline = start();
            wait_for(&socket).await;
            assert_eq!(tile(&socket, 1).await.unwrap().unwrap(), expected_tile(1));
            let e = tile(&socket, 2).await.err().unwrap();
            let problem = Problem::from_error(&e).unwrap();
            assert_eq!(problem.status, 502);
            assert_eq!(stored_names(&socket).await, Vec::<String>::new());
            terminate(offline);
        })
        .await;
}
