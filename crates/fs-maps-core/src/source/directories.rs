//! Leaf directories kept between tile lookups, within a budget.
//!
//! Pattern: Cache (FIFO) behind the crate's `DirectoryCache` — the map of the `pmtiles` crate grows
//! with every directory an archive points to, a hostile one included.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};

use pmtiles::{DirEntry, Directory, DirectoryCache, PmtResult, TileId};

const BUDGET_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Held {
    directories: HashMap<u64, Directory>,
    order: VecDeque<u64>,
    bytes: usize,
}

/// The directories of one archive.
#[derive(Default)]
pub(super) struct BoundedDirectories(Mutex<Held>);

impl BoundedDirectories {
    fn keep(&self, offset: u64, directory: Directory) {
        let size = directory.get_approx_byte_size();
        if size > BUDGET_BYTES {
            return;
        }
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        while held.bytes + size > BUDGET_BYTES {
            let Some(oldest) = held.order.pop_front() else {
                break;
            };
            if let Some(gone) = held.directories.remove(&oldest) {
                held.bytes -= gone.get_approx_byte_size();
            }
        }
        if let Some(old) = held.directories.insert(offset, directory) {
            held.bytes -= old.get_approx_byte_size();
        } else {
            held.order.push_back(offset);
        }
        held.bytes += size;
    }
}

impl DirectoryCache for BoundedDirectories {
    async fn get_dir_entry_or_insert(
        &self,
        offset: u64,
        tile_id: TileId,
        fetcher: impl Future<Output = PmtResult<Directory>> + Send,
    ) -> PmtResult<Option<DirEntry>> {
        {
            let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(directory) = held.directories.get(&offset) {
                return Ok(directory.find_tile_id(tile_id).copied());
            }
        }
        let directory = fetcher.await?;
        let entry = directory.find_tile_id(tile_id).copied();
        self.keep(offset, directory);
        Ok(entry)
    }
}
