//! Decompressing what an archive claims, never further than the limit.
//!
//! Pattern: Strategy (one per compression) + Guard — a few KB can claim to be gigabytes. The reader
//! of the `pmtiles` crate decompresses to the end, so tiles are decoded here and directories are
//! tried here first (`guard.rs`). The work runs off the serving thread.

use std::io::{self, Read};

use bytes::Bytes;
use flate2::bufread::GzDecoder;
use pmtiles::Compression;

/// Largest tile a source hands out; the same as `MAX_PAGE_BYTES` of the rpc side, checked in its test.
pub const MAX_TILE_BYTES: usize = 32 * 1024 * 1024;
/// Largest directory, decompressed. Real leaf directories stay far below it.
pub(super) const MAX_DIRECTORY_BYTES: usize = 16 * 1024 * 1024;

fn up_to(reader: impl Read, limit: usize) -> io::Result<Option<Bytes>> {
    let mut out = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut out)?;
    Ok((out.len() <= limit).then(|| Bytes::from(out)))
}

fn decode(compression: Compression, raw: &Bytes, limit: usize) -> io::Result<Option<Bytes>> {
    match compression {
        Compression::None => Ok((raw.len() <= limit).then(|| raw.clone())),
        Compression::Gzip => up_to(GzDecoder::new(&raw[..]), limit),
        Compression::Brotli => up_to(brotli::Decompressor::new(&raw[..], 4096), limit),
        other => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("{other:?} is not read"),
        )),
    }
}

/// The bytes behind `raw`; `None` if there are more than `limit`.
pub(super) async fn bounded(
    compression: Compression,
    raw: Bytes,
    limit: usize,
) -> io::Result<Option<Bytes>> {
    if compression == Compression::None {
        return decode(compression, &raw, limit);
    }
    tokio::task::spawn_blocking(move || decode(compression, &raw, limit))
        .await
        .map_err(io::Error::other)?
}
