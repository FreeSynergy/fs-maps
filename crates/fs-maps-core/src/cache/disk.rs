//! Disk layout of the cache: paths, entry files, directory scan.
//!
//! Pattern: Adapter — the file system behind the cache index. Each file is an 8-byte
//! little-endian "stored at" timestamp followed by the raw MVT bytes; recency is persisted via
//! the modification time (touched on every hit).

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::index::Key;
use crate::error::{Error, Result};

pub(super) const HEADER: usize = 8;
const FILE_EXT: &str = "mvt";

/// FNV-1a 64: stable across runs and platforms (directory names must not change).
pub fn source_hash(source_id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in source_id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub(super) fn path_of(dir: &Path, k: Key) -> PathBuf {
    dir.join(format!("{:016x}", k.0))
        .join(k.1.to_string())
        .join(k.2.to_string())
        .join(format!("{}.{FILE_EXT}", k.3))
}

pub(super) fn read_dir(p: &Path) -> Result<Vec<fs::DirEntry>> {
    Ok(fs::read_dir(p)
        .map_err(|e| Error::io(p, e))?
        .flatten()
        .collect())
}

fn read_stored_at(p: &Path) -> Option<u64> {
    let mut b = [0u8; HEADER];
    fs::File::open(p).ok()?.read_exact(&mut b).ok()?;
    Some(u64::from_le_bytes(b))
}

/// Refresh the modification time (LRU order survives restarts).
pub(super) fn touch(p: &Path) {
    if let Ok(f) = fs::OpenOptions::new().write(true).open(p) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Atomic write: temp file, then rename.
pub(super) fn write_entry(tmp: &Path, path: &Path, now: u64, data: &[u8]) -> std::io::Result<()> {
    let mut f = fs::File::create(tmp)?;
    f.write_all(&now.to_le_bytes())?;
    f.write_all(data)?;
    f.sync_data().ok();
    fs::rename(tmp, path)
}

/// One entry file found on disk: `(mtime, key, size, stored_at)`.
pub(super) type Found = (SystemTime, Key, u64, u64);

fn num<T: std::str::FromStr>(e: &fs::DirEntry) -> Option<T> {
    e.file_name().to_str().and_then(|n| n.parse().ok())
}

/// Scan the cache directory; stale temp files and truncated entries are removed.
pub(super) fn scan(dir: &Path) -> Result<Vec<Found>> {
    let mut found = Vec::new();
    for src in read_dir(dir)? {
        let Some(h) = src
            .file_name()
            .to_str()
            .and_then(|n| u64::from_str_radix(n, 16).ok())
        else {
            continue;
        };
        if !src.path().is_dir() {
            continue;
        }
        for z in read_dir(&src.path())? {
            let Some(zv) = num::<u8>(&z) else { continue };
            for x in read_dir(&z.path())? {
                let Some(xv) = num::<u32>(&x) else { continue };
                for f in read_dir(&x.path())? {
                    if let Some(item) = scan_file(&f, (h, zv, xv)) {
                        found.push(item);
                    }
                }
            }
        }
    }
    Ok(found)
}

fn scan_file(f: &fs::DirEntry, (h, zv, xv): (u64, u8, u32)) -> Option<Found> {
    let p = f.path();
    let name = f.file_name();
    let name = name.to_string_lossy();
    if name.starts_with(".tmp-") {
        let _ = fs::remove_file(&p);
        return None;
    }
    let yv = name
        .strip_suffix(&format!(".{FILE_EXT}"))?
        .parse::<u32>()
        .ok()?;
    let md = f.metadata().ok()?;
    if md.len() < HEADER as u64 {
        let _ = fs::remove_file(&p);
        return None;
    }
    let stored = read_stored_at(&p).unwrap_or(0);
    Some((
        md.modified().unwrap_or(UNIX_EPOCH),
        (h, zv, xv, yv),
        md.len(),
        stored,
    ))
}
