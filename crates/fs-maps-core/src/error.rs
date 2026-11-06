//! Crate error type.
//!
//! Pattern: Newtype-style error enum — one typed error for the whole crate, converted with `?`.

use std::path::PathBuf;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Everything that can go wrong in `fs-maps-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Tile coordinate outside the pyramid.
    #[error("invalid tile coordinate {z}/{x}/{y}")]
    InvalidTile {
        /// Zoom.
        z: u8,
        /// Column.
        x: u32,
        /// Row.
        y: u32,
    },
    /// Bounding box out of range or unordered.
    #[error("invalid bounding box: {0}")]
    InvalidBBox(String),
    /// Zoom range unordered or above the maximum.
    #[error("invalid zoom range {min}..={max}")]
    InvalidZoom {
        /// Requested minimum.
        min: u8,
        /// Requested maximum.
        max: u8,
    },
    /// PMTiles reader/writer error.
    #[error("pmtiles: {0}")]
    Pmtiles(#[from] pmtiles::PmtError),
    /// HTTP transport error.
    #[error("http: {0}")]
    Http(#[from] pmtiles::reqwest::Error),
    /// File-system error with the path involved.
    #[error("io: {path:?}: {source}")]
    Io {
        /// Path the operation touched.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A usage policy (OSMF, User-Agent) forbids the request.
    #[error("usage policy: {0}")]
    Policy(String),
    /// Region larger than the configured tile limit.
    #[error("region needs {tiles} tiles, limit is {limit}")]
    TooManyTiles {
        /// Tiles needed.
        tiles: u64,
        /// Configured limit.
        limit: u64,
    },
    /// The progress observer asked to stop.
    #[error("download cancelled")]
    Cancelled,
    /// Malformed `geo:` URI.
    #[error("invalid geo: URI: {0}")]
    GeoUri(String),
    /// Malformed GeoJSON.
    #[error("invalid GeoJSON: {0}")]
    GeoJson(String),
    /// JSON (de)serialisation error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// No place with this id.
    #[error("place {0} not found")]
    NotFound(fs_store::ObjectId),
    /// The explicit "not possible" of a provider (ADR 0045).
    #[error(transparent)]
    Unsupported(#[from] fs_interfaces::Unsupported),
    /// Error from the object store.
    #[error("store: {0}")]
    Store(#[from] fs_store::Error),
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
