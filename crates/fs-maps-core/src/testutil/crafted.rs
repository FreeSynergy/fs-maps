//! PMTiles archives put together by hand, as an attacker would.
//!
//! Pattern: Builder (test data) — no writer would produce these; the bytes are laid out here.

use std::io::Write;

/// Gzip of `data`.
pub fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    out.write_all(data).unwrap();
    out.finish().unwrap()
}

/// Brotli of `data`.
pub fn brotli(data: &[u8]) -> Vec<u8> {
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

/// A directory of one entry at tile 0. `run` 0 marks a leaf directory.
pub fn directory(run: u64, length: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for v in [1, 0, run, length, 1] {
        varint(v, &mut out);
    }
    out
}

/// A valid directory of `bytes` bytes that packs to almost nothing and parses to six times its size:
/// every entry is the same one, four bytes.
pub fn big_directory(bytes: usize) -> Vec<u8> {
    let n = (bytes - 4) / 4;
    let mut out = Vec::with_capacity(bytes);
    varint(n as u64, &mut out);
    out.resize(out.len() + n, 0); // tile id deltas
    out.resize(out.len() + n, 1); // run lengths
    out.resize(out.len() + n, 7); // lengths
    out.resize(out.len() + n, 1); // offsets: all the first tile
    out
}

/// A PMTiles v3 file: `kinds` = internal compression, tile compression, tile type.
pub fn archive(
    kinds: [u8; 3],
    root: &[u8],
    leaf: &[u8],
    data: &[u8],
    claimed_data: u64,
) -> Vec<u8> {
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
