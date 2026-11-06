//! Offline regions: download all tiles of a bbox + zoom range into a local PMTiles file.
//!
//! Pattern: Command + Iterator + Observer behind a Facade — [`RegionDownload`] is the entry
//! point; the plan iterates tiles in Hilbert order, the command runs the download and reports
//! to a [`ProgressObserver`].
//!
//! The package is a normal PMTiles v3 archive (MVT tiles, gzip), so it can be read by
//! [`crate::PmtilesFileSource`], by any PMTiles tool, and shared as a file or through Git
//! (ADR 0036). Tiles are fetched strictly one after another (polite to servers).
//!
//! Never point this at `tile.openstreetmap.org` (forbidden; see RESEARCH.md).

mod command;
mod download;
mod estimate;
mod observer;
mod plan;
mod spec;

pub use download::RegionDownload;
pub use observer::ProgressObserver;
pub use spec::{Estimate, Progress, RegionReport, RegionSpec};
