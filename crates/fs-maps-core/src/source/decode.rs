//! Decompressing what an archive claims, never further than the limit.
//!
//! Pattern: Strategy (one per compression) + Guard — a few KB can claim to be gigabytes. The reader
//! of the `pmtiles` crate decompresses to the end, so tiles are decoded here and directories are
//! tried here first (`guard.rs`). The work runs off the serving thread.

use std::io::{self, Read};

use bytes::Bytes;
use flate2::bufread::GzDecoder;
use pmtiles::Compression;

/// Largest tile a source hands out. At most `MAX_PAGE_BYTES` of the rpc side, checked in its test, and
/// lower, because answers wait in the connection's write queue (`DESIGN.md`).
pub const MAX_TILE_BYTES: usize = 8 * 1024 * 1024;
/// Largest directory, decompressed; it parses to at most six times this. A writer grows its leaves
/// until the root fits 16 KiB, about 2 500 entries: a planet of some 350 million entries has leaves of
/// about 150 000 entries, 1 to 2 MiB (reckoned, see `DESIGN.md`).
pub(super) const MAX_DIRECTORY_BYTES: usize = 4 * 1024 * 1024;

fn up_to(reader: impl Read, limit: usize) -> io::Result<Option<Bytes>> {
    // All at once: pages not written cost nothing, and growing by doubling would cost twice the limit.
    let mut out = Vec::with_capacity(limit + 1);
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// Endless zeros that count what was taken.
    struct Endless<'a>(&'a Cell<usize>);

    impl Read for Endless<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(4096);
            buf[..n].fill(0);
            self.0.set(self.0.get() + n);
            if self.0.get() > 1 << 24 {
                return Ok(0); // the limit is gone: end it, the assertion below tells
            }
            Ok(n)
        }
    }

    #[test]
    fn reading_stops_just_past_the_limit() {
        let taken = Cell::new(0);
        assert_eq!(up_to(Endless(&taken), 1000).unwrap(), None);
        assert!(taken.get() <= 64 * 1024, "took {}", taken.get());
    }

    #[test]
    fn the_limit_itself_passes() {
        let plain = Bytes::from_static(b"12345");
        let got = decode(Compression::None, &plain, 5).unwrap();
        assert_eq!(got.as_deref(), Some(&b"12345"[..]));
        assert_eq!(decode(Compression::None, &plain, 4).unwrap(), None);
        assert_eq!(
            up_to(&b"12345"[..], 5).unwrap().as_deref(),
            Some(&b"12345"[..])
        );
        assert_eq!(up_to(&b"12345"[..], 4).unwrap(), None);
    }
}
