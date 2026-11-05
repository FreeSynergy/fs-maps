use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid tile coordinate {z}/{x}/{y}")]
    InvalidTile { z: u8, x: u32, y: u32 },
    #[error("invalid bounding box: {0}")]
    InvalidBBox(String),
    #[error("invalid zoom range {min}..={max}")]
    InvalidZoom { min: u8, max: u8 },
    #[error("pmtiles: {0}")]
    Pmtiles(#[from] pmtiles::PmtError),
    #[error("http: {0}")]
    Http(#[from] pmtiles::reqwest::Error),
    #[error("io: {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("usage policy: {0}")]
    Policy(String),
    #[error("region needs {tiles} tiles, limit is {limit}")]
    TooManyTiles { tiles: u64, limit: u64 },
    #[error("download cancelled")]
    Cancelled,
    #[error("invalid geo: URI: {0}")]
    GeoUri(String),
    #[error("invalid GeoJSON: {0}")]
    GeoJson(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
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
