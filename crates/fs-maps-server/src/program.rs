//! `serve`: the server as a command of the one program (ADR 0063): `fs-maps serve [--socket PATH]
//! [--data DIR] [--actor IRI] [--instance DOMAIN]`; the tiles come from the environment
//! (`FS_MAPS_TILES`, `FS_MAPS_TILES_URL`, see `TileConfig::from_env`). No source: tiles answer
//! `Unsupported`.
//!
//! Pattern: Facade — reads its locations, opens store and tile sources, serves until a signal.

use std::ffi::OsString;
use std::sync::Arc;

use fs_interfaces::rpc::{ServerOptions, prepare_data_dir, run_until_signal};
use fs_maps_core::StorePlaces;
use fs_objects::{Store, TursoStore};

use crate::{MapsServer, TileConfig, open_tiles};

/// The help text of `fs-maps serve`.
pub const HELP: &str = "fs-maps serve [--socket PATH] [--data DIR] [--actor IRI] [--instance DOMAIN]\nTiles: $FS_MAPS_TILES (a file or folder) or $FS_MAPS_TILES_URL; without one tiles answer \"not possible\".\nDefaults: $XDG_RUNTIME_DIR/freesynergy/maps.sock, $XDG_DATA_HOME/freesynergy/maps, the local owner";

/// Why `serve` ended without serving.
#[derive(Debug, PartialEq, Eq)]
pub enum ServeError {
    /// The words or the tile settings were wrong; the caller should exit with 2.
    Usage(String),
    /// The server could not start or stopped with an error; exit with 1.
    Failed(String),
}

/// Serve `places.capnp` until a signal arrives. `args` are the words after `serve`.
///
/// # Errors
/// [`ServeError::Usage`] for unknown or incomplete words or bad tile settings,
/// [`ServeError::Failed`] for everything else.
pub fn serve(args: impl IntoIterator<Item = OsString>) -> Result<(), ServeError> {
    let Some(options) = ServerOptions::parse("maps", args).map_err(ServeError::Usage)? else {
        println!("{HELP}");
        return Ok(());
    };
    let tiles = TileConfig::from_env(&options.data).map_err(ServeError::Usage)?;
    run_until_signal(&options.socket, || async {
        let data = prepare_data_dir(&options.data)?;
        let store = TursoStore::open(data.join("objects.db"))
            .await
            .map_err(std::io::Error::other)?;
        let store: Arc<dyn Store> = Arc::new(store);
        let places = StorePlaces::new(store, options.actor.clone());
        let places = Arc::new(match &options.instance {
            Some(host) => places.with_instance(host.as_str()),
            None => places,
        });
        let tiles = open_tiles(&tiles).await.map_err(std::io::Error::other)?;
        Ok(MapsServer::new(places.clone(), places, tiles)
            .into_client()
            .client)
    })
    .map_err(|e| ServeError::Failed(e.to_string()))
}
