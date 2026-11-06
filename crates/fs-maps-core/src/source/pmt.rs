//! One opened archive: what it must be, and its tiles within the limits.
//!
//! Pattern: Adapter + Facade — translates the foreign header and reader into our `SourceInfo` and
//! tile reads, on top of the guard. `TileSource` promises uncompressed MVT, so an archive of anything
//! else is refused at open and not later per tile. What one tile costs is bounded by the guard; how
//! many at a time, whatever the backend, by the semaphore here.

use std::io;
use std::sync::Arc;

use bytes::Bytes;
use pmtiles::{AsyncBackend, AsyncPmTilesReader, Compression, Header, PmtError, TileId, TileType};
use tokio::sync::Semaphore;

use super::SourceInfo;
use super::decode::{MAX_TILE_BYTES, bounded};
use super::directories::BoundedDirectories;
use super::guard::{Facts, Guarded};
use crate::error::{Error, Result};
use crate::tile::{BBox, Tile};

const ZSTD: &str = "zstd needs a C library, FreeSynergy stays pure Rust; recompress with gzip";

fn no_zstd(e: PmtError) -> Error {
    match e {
        PmtError::UnsupportedCompression(Compression::Zstd) => Error::Archive(ZSTD.into()),
        other => other.into(),
    }
}

fn check(header: &Header) -> Result<()> {
    if header.tile_type != TileType::Mvt {
        return Err(Error::Archive(format!(
            "its tiles are {:?}, a tile source serves MVT",
            header.tile_type
        )));
    }
    match header.tile_compression {
        Compression::None | Compression::Gzip | Compression::Brotli => Ok(()),
        Compression::Zstd => Err(Error::Archive(ZSTD.into())),
        Compression::Unknown => Err(Error::Archive("its compression is unknown".into())),
    }
}

/// An archive that was checked at open.
pub(super) struct Archive<B: AsyncBackend + Sync + Send> {
    reader: AsyncPmTilesReader<Guarded<B>, BoundedDirectories>,
    facts: Arc<Facts>,
    in_flight: Semaphore,
}

/// Tiles of one archive read at the same time.
pub(super) const IN_FLIGHT: usize = 4;

impl<B: AsyncBackend + Sync + Send> Archive<B> {
    pub(super) async fn open(backend: B, in_flight: usize) -> Result<Self> {
        let facts = Arc::new(Facts::default());
        let guarded = Guarded {
            inner: backend,
            facts: facts.clone(),
        };
        let reader =
            AsyncPmTilesReader::try_from_source_and_cache(guarded, BoundedDirectories::default())
                .await
                .map_err(no_zstd)?;
        check(reader.get_header())?;
        Ok(Self {
            reader,
            facts,
            in_flight: Semaphore::new(in_flight.max(1)),
        })
    }

    pub(super) fn info(&self) -> SourceInfo {
        let h = self.reader.get_header();
        SourceInfo {
            min_zoom: h.min_zoom,
            max_zoom: h.max_zoom,
            bounds: BBox::new(
                h.min_longitude,
                h.min_latitude,
                h.max_longitude,
                h.max_latitude,
            )
            .ok(),
        }
    }

    pub(super) fn version(&self) -> Option<String> {
        self.facts.version()
    }

    pub(super) async fn tile(&self, z: u8, x: u32, y: u32) -> Result<Option<Bytes>> {
        let tile = Tile::new(z, x, y)?;
        let _permit = self
            .in_flight
            .acquire()
            .await
            .expect("semaphore never closed");
        let large = || Error::TileTooLarge { z, x, y };
        let raw = match self.reader.get_tile(TileId::from(tile.pmt()?)).await {
            Ok(raw) => raw,
            Err(PmtError::Reading(e)) if e.kind() == io::ErrorKind::FileTooLarge => {
                return Err(large());
            }
            Err(e) => return Err(e.into()),
        };
        let Some(raw) = raw else { return Ok(None) };
        let compression = self.reader.get_header().tile_compression;
        match bounded(compression, raw, MAX_TILE_BYTES).await {
            Ok(Some(tile)) => Ok(Some(tile)),
            Ok(None) => Err(large()),
            Err(e) => Err(PmtError::Reading(e).into()),
        }
    }
}
