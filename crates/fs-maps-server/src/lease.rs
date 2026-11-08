//! A server that a window started ends when no window is left (the lease).
//!
//! Pattern: Lease (a lock on a file) — a window holds a *shared* lock on `<socket>.lease` for as long
//! as it lives (the system lets go when the process ends, also if it crashes); the server it started
//! tries to take the *exclusive* lock from time to time and ends itself, with a clean shutdown, when
//! it gets it a few times in a row. Several windows share one server: it ends with the last. A server
//! started by hand (`serve` without `--lease`) never ends by itself.

use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the server looks, unless `FS_LEASE_POLL_MS` says otherwise (tests).
const POLL: Duration = Duration::from_secs(2);
/// How many looks in a row must find nobody; a window that is just starting holds the lease before it
/// starts the server, so this is only a guard against a window closing and opening again.
const UNUSED_LOOKS: u32 = 3;
/// A new server is not judged before this has passed (a window that starts it connects within it).
const YOUNG: Duration = Duration::from_secs(10);

/// Take `--lease PATH` out of `args`; the rest goes on to the normal options.
///
/// # Errors
/// `--lease` without a path.
pub fn take_lease(args: Vec<OsString>) -> Result<(Option<PathBuf>, Vec<OsString>), String> {
    let mut rest = Vec::with_capacity(args.len());
    let mut lease = None;
    let mut it = args.into_iter();
    while let Some(word) = it.next() {
        if word == "--lease" {
            lease = Some(PathBuf::from(it.next().ok_or("--lease needs a path")?));
        } else {
            rest.push(word);
        }
    }
    Ok((lease, rest))
}

/// Whether nobody holds the lease right now (and the lease file is there to ask).
#[must_use]
pub fn unused(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().create(true).append(true).open(path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            true
        }
        Err(TryLockError::WouldBlock | TryLockError::Error(_)) => false,
    }
}

/// Watch `path` on a thread of its own; when the lease is unused, end this process as a signal
/// would (`end` is called once).
pub fn watch(path: PathBuf, end: impl FnOnce() + Send + 'static) {
    let poll = std::env::var("FS_LEASE_POLL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(POLL, Duration::from_millis);
    let young = if poll < POLL { Duration::ZERO } else { YOUNG };
    let started = Instant::now();
    std::thread::spawn(move || {
        let mut looks = 0;
        loop {
            std::thread::sleep(poll);
            if started.elapsed() < young {
                continue;
            }
            if unused(&path) {
                looks += 1;
                if looks >= UNUSED_LOOKS {
                    end();
                    return;
                }
            } else {
                looks = 0;
            }
        }
    });
}

/// The holder's side: take the shared lock and keep it as long as the returned file lives.
///
/// # Errors
/// The file cannot be made or locked.
pub fn hold(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    file.lock_shared()?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lease_is_used_while_a_holder_lives_and_unused_after() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.lease");
        assert!(unused(&path), "nobody holds it yet");
        let first = hold(&path).unwrap();
        let second = hold(&path).unwrap();
        assert!(!unused(&path));
        drop(first);
        assert!(!unused(&path), "one holder is left");
        drop(second);
        assert!(unused(&path));
    }

    #[test]
    fn the_lease_option_is_taken_out_and_needs_a_path() {
        let words: Vec<OsString> = ["--lease", "/x", "--socket", "s"].map(Into::into).to_vec();
        let (lease, rest) = take_lease(words).unwrap();
        assert_eq!(lease, Some(PathBuf::from("/x")));
        assert_eq!(rest, ["--socket", "s"].map(OsString::from));
        assert!(take_lease(vec!["--lease".into()]).is_err());
    }
}
