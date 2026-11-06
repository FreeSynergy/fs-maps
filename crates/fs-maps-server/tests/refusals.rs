//! The real program refusing to start: exit codes, and what it leaves behind.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

fn paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("maps.sock"), dir.join("data"))
}

#[test]
fn a_program_refuses_what_it_cannot_serve_with_the_right_code() {
    let dir = tempfile::tempdir().unwrap();
    let (socket, data) = paths(dir.path());
    let code = |args: &[&str], env: &[(&str, &str)]| {
        let mut command = program(&socket, &data);
        command.args(args);
        for (k, v) in env {
            command.env(k, v);
        }
        command.status().unwrap().code()
    };
    assert_eq!(code(&["--nonsense"], &[]), Some(2));
    assert_eq!(code(&["--actor"], &[]), Some(2));
    assert_eq!(
        code(&["--tiles", "x"], &[]),
        Some(2),
        "that is the environment's"
    );
    let url = ("FS_MAPS_TILES_URL", "https://example.invalid/a.pmtiles");
    assert_eq!(code(&[], &[url, ("FS_MAPS_CACHE_MIB", "many")]), Some(2));
    assert_eq!(
        code(&[], &[("FS_MAPS_TILES", "/nonexistent.pmtiles")]),
        Some(1)
    );
    let forbidden = (
        "FS_MAPS_TILES_URL",
        "https://tile.openstreetmap.org/x.pmtiles",
    );
    assert_eq!(code(&[], &[forbidden]), Some(1));
    let user = (
        "FS_MAPS_TILES_URL",
        "https://me:secret@example.invalid/a.pmtiles",
    );
    assert_eq!(code(&[], &[user]), Some(1));
    let dotted = (
        "FS_MAPS_TILES_URL",
        "https://tile.openstreetmap.org./x.pmtiles",
    );
    assert_eq!(code(&[], &[dotted]), Some(1));
    let agent = ("FS_MAPS_USER_AGENT", "A long enough agent\nwith a newline");
    assert_eq!(code(&[], &[url, agent]), Some(1));
    let pictures = dir.path().join("pictures.pmtiles");
    let mut bytes = b"PMTiles\x03".to_vec();
    bytes.resize(16384, 0);
    std::fs::write(&pictures, bytes).unwrap();
    assert_eq!(
        code(&[], &[("FS_MAPS_TILES", pictures.to_str().unwrap())]),
        Some(1)
    );
    assert!(!socket.exists(), "a refused start left its socket behind");
    assert_eq!(code(&["--help"], &[]), Some(0));
}

#[test]
fn a_data_directory_others_can_reach_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (socket, data) = paths(dir.path());
    std::fs::create_dir(&data).unwrap();
    std::fs::set_permissions(&data, PermissionsExt::from_mode(0o755)).unwrap();
    assert_eq!(program(&socket, &data).status().unwrap().code(), Some(1));
    assert!(
        !data.join("objects.db").exists(),
        "it opened the store anyway"
    );
    assert!(!socket.exists());
}

#[test]
fn a_cache_others_can_reach_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (socket, data) = paths(dir.path());
    let cache = dir.path().join("cache");
    std::fs::create_dir(&cache).unwrap();
    std::fs::set_permissions(&cache, PermissionsExt::from_mode(0o755)).unwrap();
    let mut child = program(&socket, &data)
        .env("FS_MAPS_TILES_URL", "https://example.invalid/a.pmtiles")
        .env("FS_MAPS_CACHE", &cache)
        .spawn()
        .unwrap();
    // A server that wrongly starts never exits: that must fail here, not hang.
    for _ in 0..400 {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(1));
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    panic!("it started with a cache others can reach");
}
