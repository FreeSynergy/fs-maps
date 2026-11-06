//! No tile source configured.
//!
//! Pattern: Null Object — a program without a configured map still has the `TileSource` interface;
//! it answers the explicit "not possible" of the provider rule (ADR 0045) instead of "no such tile",
//! which would look like an empty map.

use async_trait::async_trait;
use bytes::Bytes;
use fs_interfaces::{Capabilities, Unsupported};

use super::TileSource;
use crate::error::Result;

const REASON: &str = "no tile source is configured";

/// The source of a program nobody gave a map to.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoSource;

#[async_trait]
impl TileSource for NoSource {
    fn id(&self) -> String {
        "none".into()
    }
    async fn get_tile(&self, _z: u8, _x: u32, _y: u32) -> Result<Option<Bytes>> {
        Err(Unsupported::new("tile.read", REASON).into())
    }
    fn capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new("tiles:none");
        caps.unsupported("tile.read", REASON);
        caps
    }
}
