//! Peak memory of the real program against archives built to cost it, with many calls at once. This
//! reads what the process really reached (`VmHWM`).

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use fs_interfaces::places_capnp::places;
use fs_interfaces::rpc::connect;
use fs_maps_core::testutil::{archive, big_directory, directory, gzip};

const NONE: u8 = 1;
const GZIP: u8 = 2;
const MVT: u8 = 1;
const MIB: usize = 1024 * 1024;
const LIMIT_MIB: usize = 400;

fn peak_mib(child: &Child) -> usize {
    let status = std::fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap();
    let line = status.lines().find(|l| l.starts_with("VmHWM:")).unwrap();
    let kib: usize = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    kib / 1024
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

/// Serve `bytes` as the map, ask for its tile `calls` times at once; `(peak MiB, answers that failed)`.
async fn under_load(bytes: &[u8], calls: usize) -> (usize, usize) {
    let dir = tempfile::tempdir().unwrap();
    let (socket, data) = (dir.path().join("maps.sock"), dir.path().join("data"));
    let file = dir.path().join("hostile.pmtiles");
    std::fs::write(&file, bytes).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fs-maps-server"))
        .arg("--socket")
        .arg(&socket)
        .arg("--data")
        .arg(&data)
        .env("FS_MAPS_TILES", &file)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&socket).await;
    let connection = connect::<places::Client>(&socket).await.unwrap();
    let tiles = connection
        .client
        .tiles_request()
        .send()
        .pipeline
        .get_source();
    let mut waiting = Vec::new();
    for _ in 0..calls {
        let request = tiles.tile_request();
        waiting.push(request.send().promise);
    }
    let mut failed = 0;
    for call in waiting {
        if call.await.is_err() {
            failed += 1;
        }
    }
    let peak = peak_mib(&child);
    child.kill().unwrap();
    child.wait().unwrap();
    (peak, failed)
}

fn run(test: impl Future<Output = ()>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, test);
}

#[test]
fn a_tile_bomb_costs_a_bound_however_many_ask() {
    run(async {
        let tile = gzip(&vec![0u8; 64 * MIB]);
        assert!(tile.len() < MIB);
        let root = gzip(&directory(1, tile.len() as u64));
        let bytes = archive([GZIP, GZIP, MVT], &root, &[], &tile, tile.len() as u64);
        let (peak, failed) = under_load(&bytes, 64).await;
        println!("tile bomb, 64 calls: peak {peak} MiB");
        assert_eq!(failed, 64, "the tile was served");
        assert!(peak < LIMIT_MIB, "peak {peak} MiB");
    });
}

#[test]
fn a_directory_bomb_costs_a_bound_however_many_ask() {
    run(async {
        // At the limit it is valid, and parsed; just over it, it is refused every time.
        for (bytes, calls, refused) in [(4 * MIB, 256, 0), (4 * MIB + 8, 256, 256)] {
            let leaf = gzip(&big_directory(bytes));
            assert!(leaf.len() < MIB / 4);
            let root = gzip(&directory(0, leaf.len() as u64));
            let hostile = archive([GZIP, NONE, MVT], &root, &leaf, b"reached", 7);
            let (peak, failed) = under_load(&hostile, calls).await;
            println!("directory of {bytes} bytes, {calls} calls: peak {peak} MiB");
            assert_eq!(failed, refused);
            assert!(peak < LIMIT_MIB, "peak {peak} MiB");
        }
    });
}
