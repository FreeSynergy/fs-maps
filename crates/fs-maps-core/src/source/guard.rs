//! What the `pmtiles` reader may read.
//!
//! Pattern: Decorator over `AsyncBackend` — refuses a read larger than a tile may be, before it
//! happens, and tries every directory with a limit before the reader decompresses it without one.
//! The reader keeps the layout of the archive to itself, so it is read here from the fixed v3 header.

use std::io;
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use pmtiles::{AsyncBackend, BackendResponse, Compression, PmtError, PmtResult};

use super::decode::{MAX_DIRECTORY_BYTES, MAX_TILE_BYTES, bounded};

const HEADER_BYTES: usize = 127;

fn word(header: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(header.get(at..at + 8)?.try_into().ok()?))
}

#[derive(Clone, Copy)]
struct Layout {
    root: (u64, u64),
    leaves: (u64, u64),
    internal: Compression,
}

impl Layout {
    fn read(header: &[u8]) -> Option<Self> {
        if header.len() < HEADER_BYTES {
            return None;
        }
        Some(Self {
            root: (word(header, 8)?, word(header, 16)?),
            leaves: (word(header, 40)?, word(header, 48)?),
            internal: Compression::try_from(*header.get(97)?).ok()?,
        })
    }

    fn is_leaf(&self, offset: u64) -> bool {
        offset >= self.leaves.0 && offset - self.leaves.0 < self.leaves.1
    }
}

/// What the first read told about the archive.
#[derive(Default)]
pub(super) struct Facts {
    layout: OnceLock<Layout>,
    version: OnceLock<String>,
}

impl Facts {
    /// `ETag` or `Last-Modified` of the first read, if the backend has one.
    pub(super) fn version(&self) -> Option<String> {
        self.version.get().cloned()
    }
}

/// The error of a read larger than a tile may be; `pmt.rs` knows it again.
pub(super) fn too_large() -> PmtError {
    PmtError::Reading(io::ErrorKind::FileTooLarge.into())
}

async fn check_directory(compression: Compression, raw: Bytes) -> PmtResult<()> {
    // What cannot be read here the reader refuses itself, with its own words.
    if matches!(compression, Compression::Zstd | Compression::Unknown) {
        return Ok(());
    }
    match bounded(compression, raw, MAX_DIRECTORY_BYTES).await? {
        Some(_) => Ok(()),
        None => Err(PmtError::Reading(io::Error::new(
            io::ErrorKind::InvalidData,
            "a directory of the archive is too large",
        ))),
    }
}

/// The backend `B` behind the limits.
pub(super) struct Guarded<B> {
    pub(super) inner: B,
    pub(super) facts: Arc<Facts>,
}

impl<B: AsyncBackend + Sync + Send> Guarded<B> {
    async fn first_read(&self, response: &BackendResponse) -> PmtResult<()> {
        if let Some(version) = &response.data_version_string {
            let _ = self.facts.version.set(version.clone());
        }
        let Some(layout) = Layout::read(&response.bytes) else {
            return Ok(()); // the reader says what is wrong with it
        };
        let _ = self.facts.layout.set(layout);
        let start = usize::try_from(layout.root.0).ok();
        let end = start.zip(usize::try_from(layout.root.1).ok());
        let end = end.and_then(|(s, l)| s.checked_add(l));
        if let (Some(start), Some(end)) = (start, end)
            && start <= end
            && end <= response.bytes.len()
        {
            check_directory(layout.internal, response.bytes.slice(start..end)).await?;
        }
        Ok(())
    }
}

impl<B: AsyncBackend + Sync + Send> AsyncBackend for Guarded<B> {
    async fn read(&self, offset: u64, length: usize) -> PmtResult<BackendResponse> {
        if length > MAX_TILE_BYTES {
            return Err(too_large());
        }
        let response = self.inner.read(offset, length).await?;
        if offset == 0 {
            self.first_read(&response).await?;
        } else if let Some(layout) = self.facts.layout.get()
            && layout.is_leaf(offset)
        {
            check_directory(layout.internal, response.bytes.clone()).await?;
        }
        Ok(response)
    }
}
