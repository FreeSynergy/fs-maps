//! A leaf section that claims to begin at offset 0, where the header is.

use fs_maps_core::testutil::{RangeServer, gzip, leaf_at_zero};
use fs_maps_core::{LazyHttpSource, PmtilesFileSource, TileSource};

const UA: &str = "FreeSynergy-Maps-test/0.2 (+https://freesynergy.example)";

#[tokio::test]
async fn a_server_answering_a_later_read_at_zero_with_a_bomb_is_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let bomb = gzip(&vec![0u8; 64 * 1024 * 1024]);
    assert_ne!(
        bomb.len(),
        16384,
        "the server tells the reads apart by their length"
    );
    let file = dir.path().join("zero.pmtiles");
    std::fs::write(&file, leaf_at_zero(bomb.len() as u64)).unwrap();
    let server = RangeServer::start(&file).await.unwrap();
    server.answer_at_zero(bomb);
    let source = LazyHttpSource::new(&server.url, UA).unwrap();
    let e = source.get_tile(0, 0, 0).await.unwrap_err();
    assert!(
        e.to_string()
            .contains("directory of the archive is too large"),
        "{e}"
    );
}

#[tokio::test]
async fn in_a_local_file_offset_0_can_only_be_the_header() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("zero.pmtiles");
    std::fs::write(&file, leaf_at_zero(64)).unwrap();
    let source = PmtilesFileSource::open(&file).await.unwrap();
    // The same bytes as the first read, so no bomb: they are no gzip, and that is all it comes to.
    let e = source.get_tile(0, 0, 0).await.unwrap_err();
    assert!(!e.to_string().contains("too large"), "{e}");
}
