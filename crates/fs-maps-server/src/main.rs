//! `fs-maps-server`. See [`USAGE`] for the arguments.
//!
//! Pattern: none (thin binary) — reads its locations, opens store and tile sources, serves until a
//! signal.

use std::path::PathBuf;
use std::sync::Arc;

use fs_interfaces::rpc::{default_socket, run_until_signal};
use fs_maps_core::{CacheConfig, StorePlaces};
use fs_maps_server::{MapsServer, TileConfig, open_tiles};
use fs_store::{Store, TursoStore};

const USAGE: &str = "fs-maps-server [--socket PATH] [--data DIR] [--actor IRI]
               [--tiles FILE]... [--tiles-url URL] [--user-agent TEXT]
               [--cache DIR] [--cache-mib N]
Defaults: $XDG_RUNTIME_DIR/freesynergy/maps.sock, $XDG_DATA_HOME/freesynergy/maps, the local owner.
--tiles is repeatable: local PMTiles files, asked first. --tiles-url is a remote PMTiles archive, read
through a cache (default $XDG_CACHE_HOME/freesynergy/maps, 1024 MiB). No source: tiles answer Unsupported.";
const LOCAL_OWNER: &str = "https://local.invalid/users/owner";
const DEFAULT_CACHE_MIB: u64 = 1024;
const DEFAULT_USER_AGENT: &str = concat!(
    "FreeSynergy-Maps/",
    env!("CARGO_PKG_VERSION"),
    " (+https://freesynergy.net)"
);

fn under_home(variable: &str, fallback: &str) -> Option<PathBuf> {
    let base = std::env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)))?;
    Some(base.join("freesynergy").join("maps"))
}

fn fail(message: &str) -> ! {
    eprintln!("fs-maps-server: {message}");
    std::process::exit(2);
}

fn value(args: &mut impl Iterator<Item = String>, name: &str) -> String {
    args.next()
        .unwrap_or_else(|| fail(&format!("{name} needs a value")))
}

fn main() {
    let mut socket = default_socket("maps");
    let mut data = under_home("XDG_DATA_HOME", ".local/share");
    let mut cache = under_home("XDG_CACHE_HOME", ".cache");
    let mut cache_mib = DEFAULT_CACHE_MIB;
    let mut actor = LOCAL_OWNER.to_string();
    let mut tiles = TileConfig {
        user_agent: DEFAULT_USER_AGENT.into(),
        ..TileConfig::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => socket = Some(value(&mut args, "--socket").into()),
            "--data" => data = Some(value(&mut args, "--data").into()),
            "--actor" => actor = value(&mut args, "--actor"),
            "--tiles" => tiles.files.push(value(&mut args, "--tiles").into()),
            "--tiles-url" => tiles.url = Some(value(&mut args, "--tiles-url")),
            "--user-agent" => tiles.user_agent = value(&mut args, "--user-agent"),
            "--cache" => cache = Some(value(&mut args, "--cache").into()),
            "--cache-mib" => {
                cache_mib = value(&mut args, "--cache-mib")
                    .parse()
                    .unwrap_or_else(|_| fail("--cache-mib needs a number"));
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => fail(&format!("unknown argument {other}")),
        }
    }
    let socket = socket.unwrap_or_else(|| fail("no $XDG_RUNTIME_DIR: pass --socket"));
    let data = data.unwrap_or_else(|| fail("no $XDG_DATA_HOME or $HOME: pass --data"));
    if tiles.url.is_some() {
        let dir = cache.unwrap_or_else(|| fail("no $XDG_CACHE_HOME or $HOME: pass --cache"));
        tiles.cache = Some(CacheConfig::new(dir, cache_mib.saturating_mul(1024 * 1024)));
    }
    let served = run_until_signal(&socket, || async move {
        std::fs::create_dir_all(&data)?;
        let store = TursoStore::open(data.join("objects.db"))
            .await
            .map_err(std::io::Error::other)?;
        let store: Arc<dyn Store> = Arc::new(store);
        let places = Arc::new(StorePlaces::new(store, actor));
        let tiles = open_tiles(&tiles).await.map_err(std::io::Error::other)?;
        Ok(MapsServer::new(places.clone(), places, tiles).into_client())
    });
    if let Err(e) = served {
        eprintln!("fs-maps-server: {e}");
        std::process::exit(1);
    }
}
