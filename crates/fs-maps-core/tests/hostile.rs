//! Archives that lie: bounded memory, and what is not MVT is refused at open.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use fs_maps_core::testutil::write_fixture;
use fs_maps_core::{
    BBox, Error, MAX_TILE_BYTES, MemorySource, PmtilesFileSource, RegionDownload, RegionSpec,
    TileSource,
};

const NONE: u8 = 1; // and below: as numbered in the PMTiles header
const GZIP: u8 = 2;
const BROTLI: u8 = 3;
const ZSTD: u8 = 4;
const MVT: u8 = 1;
const PNG: u8 = 2;

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    out.write_all(data).unwrap();
    out.finish().unwrap()
}

fn brotli(data: &[u8]) -> Vec<u8> {
    let mut w = brotli::CompressorWriter::new(Vec::new(), 4096, 1, 20);
    w.write_all(data).unwrap();
    w.into_inner()
}

fn varint(mut v: u64, out: &mut Vec<u8>) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// One entry at tile 0. `run` 0 marks a leaf directory.
fn directory(run: u64, length: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [1, 0, run, length, 1] {
        varint(v, &mut out);
    }
    out
}

/// A PMTiles v3 file put together by hand, as an attacker would.
fn archive(kinds: [u8; 3], root: &[u8], leaf: &[u8], data: &[u8], claimed_data: u64) -> Vec<u8> {
    let [internal, tiles, tile_type] = kinds;
    let mut out = b"PMTiles\x03".to_vec();
    let (root_at, leaf_at) = (127u64, 127 + root.len() as u64);
    let data_at = leaf_at + leaf.len() as u64;
    for v in [
        root_at,
        root.len() as u64,
        data_at,
        0,
        leaf_at,
        leaf.len() as u64,
        data_at,
        claimed_data,
        1,
        1,
        1,
    ] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&[0, internal, tiles, tile_type, 0, 0]);
    out.resize(127, 0);
    out.extend_from_slice(root);
    out.extend_from_slice(leaf);
    out.extend_from_slice(data);
    out
}

async fn open(dir: &Path, bytes: &[u8]) -> fs_maps_core::Result<PmtilesFileSource> {
    let path = dir.join("hostile.pmtiles");
    std::fs::write(&path, bytes).unwrap();
    PmtilesFileSource::open(&path).await
}

async fn with_one_tile(path: &Path, bytes: usize) {
    let mut memory = MemorySource::new("big");
    memory.insert(0, 0, 0, vec![0u8; bytes]);
    let spec = RegionSpec {
        name: "big".into(),
        bbox: BBox::new(-180.0, -85.0, 180.0, 85.0).unwrap(),
        min_zoom: 0,
        max_zoom: 0,
        attribution: None,
    };
    RegionDownload::new(Arc::new(memory), spec)
        .unwrap()
        .download(path, &mut |_| true)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_tile_stops_at_the_limit_however_small_it_is_compressed() {
    let dir = tempfile::tempdir().unwrap();
    let bomb = dir.path().join("bomb.pmtiles");
    with_one_tile(&bomb, MAX_TILE_BYTES + 1).await;
    assert!(std::fs::metadata(&bomb).unwrap().len() < 1024 * 1024);
    let source = PmtilesFileSource::open(&bomb).await.unwrap();
    let e = source.get_tile(0, 0, 0).await.unwrap_err();
    assert!(
        matches!(e, Error::TileTooLarge { z: 0, x: 0, y: 0 }),
        "{e:?}"
    );

    let largest = dir.path().join("largest.pmtiles");
    with_one_tile(&largest, MAX_TILE_BYTES).await;
    let source = PmtilesFileSource::open(&largest).await.unwrap();
    let tile = source.get_tile(0, 0, 0).await.unwrap().unwrap();
    assert_eq!(tile.len(), MAX_TILE_BYTES);
}

#[tokio::test]
async fn an_entry_claiming_a_gigabyte_is_never_read() {
    let dir = tempfile::tempdir().unwrap();
    let gigabyte = 1u64 << 30;
    let bytes = archive(
        [NONE, NONE, MVT],
        &directory(1, gigabyte),
        &[],
        b"x",
        gigabyte,
    );
    let source = open(dir.path(), &bytes).await.unwrap();
    let e = source.get_tile(0, 0, 0).await.unwrap_err();
    assert!(matches!(e, Error::TileTooLarge { .. }), "{e:?}");
}

#[tokio::test]
async fn a_leaf_directory_stops_at_its_limit() {
    let dir = tempfile::tempdir().unwrap();
    let leaf = gzip(&vec![0u8; 100 << 20]);
    let root_gz = gzip(&directory(0, leaf.len() as u64));
    let bytes = archive([GZIP, NONE, MVT], &root_gz, &leaf, b"x", 1);
    let source = open(dir.path(), &bytes).await.unwrap();
    let e = source.get_tile(0, 0, 0).await.unwrap_err();
    assert!(
        e.to_string()
            .contains("directory of the archive is too large"),
        "{e}"
    );
}

#[tokio::test]
async fn a_root_directory_stops_at_its_limit() {
    let dir = tempfile::tempdir().unwrap();
    let root = brotli(&vec![0u8; 32 * 1024 * 1024]);
    assert!(root.len() < 16 * 1024);
    let bytes = archive([BROTLI, NONE, MVT], &root, &[], b"x", 1);
    let e = open(dir.path(), &bytes).await.err().unwrap();
    assert!(
        e.to_string()
            .contains("directory of the archive is too large"),
        "{e}"
    );
}

#[tokio::test]
async fn brotli_tiles_come_out_uncompressed() {
    let dir = tempfile::tempdir().unwrap();
    let tile = brotli(b"an MVT tile");
    let root = brotli(&directory(1, tile.len() as u64));
    let bytes = archive([BROTLI, BROTLI, MVT], &root, &[], &tile, tile.len() as u64);
    let source = open(dir.path(), &bytes).await.unwrap();
    assert_eq!(
        source.get_tile(0, 0, 0).await.unwrap().unwrap(),
        &b"an MVT tile"[..]
    );
}

#[tokio::test]
async fn only_mvt_in_a_compression_we_read_opens() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("good.pmtiles");
    write_fixture(&good, 1).await.unwrap();
    let fixture = std::fs::read(&good).unwrap();
    assert!(PmtilesFileSource::open(&good).await.is_ok());
    for (at, value, says) in [
        (99, PNG, "Png"),
        (99, 0, "Unknown"),
        (98, ZSTD, "zstd"),
        (98, 0, "unknown"),
        (97, ZSTD, "zstd"),
    ] {
        let mut bytes = fixture.clone();
        bytes[at] = value;
        let e = open(dir.path(), &bytes).await.err().unwrap();
        assert!(matches!(e, Error::Archive(_)), "{at}: {e:?}");
        assert!(e.to_string().contains(says), "{at}: {e}");
    }
}
