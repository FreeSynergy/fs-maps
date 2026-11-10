//! `fs-maps --version` prints the version and exits 0 (the release smoke test starts every binary that way).

#[test]
fn version_prints_and_exits_zero() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_fs-maps"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("fs-maps {}", env!("CARGO_PKG_VERSION"))
    );
}
