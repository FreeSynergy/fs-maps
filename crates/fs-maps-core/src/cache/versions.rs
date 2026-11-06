//! The version of each source's data, and tiles kept past their time.
//!
//! Pattern: Repository (internal) — `versions/<hash of the source>` holds the last version seen, so
//! after a restart while offline the cache still knows which of its tiles to answer with.

use std::fs;

use bytes::Bytes;

use super::TileCache;
use super::disk::{HEADER, make_dirs, path_of, source_hash};

const VERSIONS: &str = "versions";

impl TileCache {
    /// Remember what `version` the data of `source_id` has now.
    pub fn remember_version(&self, source_id: &str, version: &str) {
        let hash = source_hash(source_id);
        let mut known = self.versions.lock().expect("versions lock");
        if known.get(&hash).is_some_and(|v| v == version) {
            return;
        }
        known.insert(hash, version.to_owned());
        let dir = self.dir().join(VERSIONS);
        if make_dirs(&dir).is_ok() {
            // Without the note only an offline restart loses its tiles.
            let _ = fs::write(dir.join(format!("{hash:016x}")), version);
        }
    }

    /// The last version of `source_id` seen, also before this start.
    pub fn known_version(&self, source_id: &str) -> Option<String> {
        let hash = source_hash(source_id);
        let mut known = self.versions.lock().expect("versions lock");
        if let Some(version) = known.get(&hash) {
            return Some(version.clone());
        }
        let path = self.dir().join(VERSIONS).join(format!("{hash:016x}"));
        let version = fs::read_to_string(path).ok()?;
        known.insert(hash, version.clone());
        Some(version)
    }

    /// A tile regardless of its age, for when its source is away.
    pub fn get_stale(&self, source_id: &str, z: u8, x: u32, y: u32) -> Option<Bytes> {
        let key = (source_hash(source_id), z, x, y);
        let g = self.inner.lock().expect("cache lock");
        g.map.get(&key)?;
        let mut data = fs::read(path_of(&self.dir(), key)).ok()?;
        (data.len() >= HEADER).then(|| Bytes::from(data.split_off(HEADER)))
    }

    /// The source no longer has this tile.
    pub fn forget(&self, source_id: &str, z: u8, x: u32, y: u32) {
        let key = (source_hash(source_id), z, x, y);
        let mut g = self.inner.lock().expect("cache lock");
        if g.remove(&key).is_some() {
            let _ = fs::remove_file(path_of(&self.dir(), key));
        }
    }
}
