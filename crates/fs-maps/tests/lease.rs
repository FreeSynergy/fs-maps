//! A server a window started ends when no window holds the lease; one started by hand does not.
//!
//! Tests: the real binary; the polling is made fast with `FS_LEASE_POLL_MS`.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

async fn wait_for(socket: &std::path::Path) {
    for _ in 0..200 {
        if socket.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server did not come up");
}

fn serve(dir: &std::path::Path, lease: Option<&std::path::Path>) -> std::process::Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fs-maps"));
    command
        .arg("serve")
        .arg("--socket")
        .arg(dir.join("maps.sock"))
        .arg("--data")
        .arg(dir.join("data"))
        .env("FS_LEASE_POLL_MS", "100")
        .stderr(Stdio::null());
    if let Some(lease) = lease {
        command.arg("--lease").arg(lease);
    }
    command.spawn().unwrap()
}

fn ends_within(child: &mut std::process::Child, limit: Duration) -> Option<i32> {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code();
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[tokio::test(flavor = "current_thread")]
async fn the_server_ends_with_its_last_window_and_not_before() {
    let dir = tempfile::tempdir().unwrap();
    let lease = dir.path().join("maps.lease");
    // Two windows hold the lease.
    let one = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&lease)
        .unwrap();
    one.lock_shared().unwrap();
    let two = std::fs::OpenOptions::new()
        .append(true)
        .open(&lease)
        .unwrap();
    two.lock_shared().unwrap();
    let mut server = serve(dir.path(), Some(&lease));
    wait_for(&dir.path().join("maps.sock")).await;

    drop(one);
    assert_eq!(
        ends_within(&mut server, Duration::from_millis(1200)),
        None,
        "one window is left"
    );
    drop(two);
    assert_eq!(
        ends_within(&mut server, Duration::from_secs(10)),
        Some(0),
        "no window left: a clean end"
    );
    assert!(
        !dir.path().join("maps.sock").exists(),
        "the socket is removed"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_server_started_by_hand_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut server = serve(dir.path(), None);
    wait_for(&dir.path().join("maps.sock")).await;
    assert_eq!(ends_within(&mut server, Duration::from_millis(1500)), None);
    let _ = server.kill();
    let _ = server.wait();
}
