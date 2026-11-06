//! `fs-maps-server [--socket PATH] [--data DIR] [--actor IRI] [--instance DOMAIN]`; the tiles come from the environment
//! (`FS_MAPS_TILES`, `FS_MAPS_TILES_URL`, see `TileConfig::from_env`). No source: tiles answer
//! `Unsupported`.
//!
//! Pattern: none (thin binary) — reads its locations, opens store and tile sources, serves until a
//! signal.

use std::sync::Arc;

use fs_interfaces::rpc::{ServerOptions, prepare_data_dir, run_until_signal};
use fs_maps_core::StorePlaces;
use fs_maps_server::{MapsServer, TileConfig, open_tiles};
use fs_store::{Store, TursoStore};

fn main() {
    let options = ServerOptions::from_env_or_exit("maps");
    let tiles = TileConfig::from_env(&options.data).unwrap_or_else(|problem| {
        eprintln!("fs-maps-server: {problem}");
        std::process::exit(2);
    });
    let served = run_until_signal(&options.socket, || async {
        prepare_data_dir(&options.data)?;
        let store = TursoStore::open(options.data.join("objects.db"))
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
    });
    if let Err(e) = served {
        eprintln!("fs-maps-server: {e}");
        std::process::exit(1);
    }
}
