//! The real program with a remote archive on the loopback: its cache across a restart.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::{Problem, connect};
use fs_maps_core::testutil::{RangeServer, write_fixture};

fn program(socket: &Path, data: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fs-maps"));
    command
        .arg("serve")
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
            let mode = std::fs::metadata(&cache).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "the cache is not private");
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

#[tokio::test(flavor = "current_thread")]
async fn each_data_directory_has_its_own_default_cache_and_a_relative_base_is_ignored() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let dir = tempfile::tempdir().unwrap();
            let archive = dir.path().join("remote.pmtiles");
            write_fixture(&archive, 1).await.unwrap();
            let server = RangeServer::start(&archive).await.unwrap();
            let home = dir.path().join("home");
            let base = home.join(".cache/freesynergy/maps");
            let mut seen = Vec::new();
            for name in ["one", "two"] {
                let socket = dir.path().join(format!("{name}.sock"));
                let data = dir.path().join(name);
                let child = program(&socket, &data)
                    .current_dir(dir.path())
                    .env("FS_MAPS_TILES_URL", &server.url)
                    .env("XDG_CACHE_HOME", "relative-cache")
                    .env("HOME", &home)
                    .spawn()
                    .unwrap();
                wait_for(&socket).await;
                assert_eq!(tile(&socket, 1).await.unwrap().unwrap(), expected_tile(1));
                terminate(child);
                seen.push(std::fs::read_dir(&base).unwrap().count());
            }
            assert_eq!(seen, [1, 2], "two data directories shared a cache");
            assert!(!dir.path().join("relative-cache").exists());
            for entry in std::fs::read_dir(&base).unwrap() {
                let mode = entry.unwrap().metadata().unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o700);
            }
        })
        .await;
}
