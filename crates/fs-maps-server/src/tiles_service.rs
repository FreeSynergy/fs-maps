//! `TileSource` interface: where map tiles come from, one tile per call.
//!
//! Pattern: Adapter — wire request in, core `TileSource` call, wire response out. A tile is one MVT
//! and cannot be cut, so one that does not fit a message is refused, not truncated.

use std::rc::Rc;
use std::sync::Arc;

use fs_interfaces::places_capnp::tile_source;
use fs_interfaces::rpc::MAX_PAGE_BYTES;
use fs_maps_core::TileSource;

use crate::errors::{rpc_error, too_large};
use crate::wire::CONTRACT_VERSION;

pub(crate) struct TilesImpl {
    pub(crate) source: Arc<dyn TileSource>,
}

type Done = Result<(), capnp::Error>;

impl tile_source::Server for TilesImpl {
    async fn support_report(
        self: Rc<Self>,
        _: tile_source::SupportReportParams,
        mut results: tile_source::SupportReportResults,
    ) -> Done {
        let report = self.source.capabilities();
        report.write(CONTRACT_VERSION, results.get().init_report());
        Ok(())
    }

    async fn id(
        self: Rc<Self>,
        _: tile_source::IdParams,
        mut results: tile_source::IdResults,
    ) -> Done {
        results.get().set_id(self.source.id().as_str());
        Ok(())
    }

    async fn tile(
        self: Rc<Self>,
        params: tile_source::TileParams,
        mut results: tile_source::TileResults,
    ) -> Done {
        let request = params.get()?;
        let (z, x, y) = (request.get_z(), request.get_x(), request.get_y());
        let found = self.source.get_tile(z, x, y).await.map_err(rpc_error)?;
        let mut answer = results.get();
        answer.set_found(found.is_some());
        if let Some(mvt) = found {
            if mvt.len() > MAX_PAGE_BYTES {
                return Err(too_large(format!(
                    "tile {z}/{x}/{y} has {} bytes, one message holds {MAX_PAGE_BYTES}",
                    mvt.len()
                )));
            }
            answer.set_mvt(&mvt);
        }
        Ok(())
    }
}
