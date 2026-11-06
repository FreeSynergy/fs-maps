//! PMTiles fixture generator.
//!
//! Pattern: Builder (test data) — writes a small archive with the same writer the region
//! download uses.

use std::path::Path;
use std::sync::Arc;

use crate::error::Result;
use crate::region::{RegionDownload, RegionSpec};
use crate::source::MemorySource;
use crate::tile::BBox;

/// Writes a small PMTiles fixture (zoom 0..=`max_zoom`, every tile present, payload = its
/// coordinate plus filler) built with the same writer the region download uses.
pub async fn write_fixture(path: &Path, max_zoom: u8) -> Result<()> {
    write_labelled(path, max_zoom, "MVT-FIXTURE").await
}

/// [`write_fixture`], each payload beginning with `label`: tells two versions of an archive apart.
pub async fn write_labelled(path: &Path, max_zoom: u8, label: &str) -> Result<()> {
    let mut m = MemorySource::new("fixture");
    for z in 0..=max_zoom {
        for x in 0..(1u32 << z) {
            for y in 0..(1u32 << z) {
                m.insert(
                    z,
                    x,
                    y,
                    format!("{label} {z}/{x}/{y} {}", "~".repeat(200)).into_bytes(),
                );
            }
        }
    }
    let spec = RegionSpec {
        name: "fixture".into(),
        bbox: BBox::new(-180.0, -85.0, 180.0, 85.0)?,
        min_zoom: 0,
        max_zoom,
        attribution: None,
    };
    RegionDownload::new(Arc::new(m), spec)?
        .download(path, &mut |_| true)
        .await?;
    Ok(())
}
