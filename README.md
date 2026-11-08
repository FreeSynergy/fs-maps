# FreeSynergy Maps — know where

**Everybody loads their own map, and keeps it when the network is gone.**

FreeSynergy Maps is the part that brings map data to your device and keeps places you care about. It loads a map piece by piece, only what you look at. It stores pieces on your disk with a size limit you set. You can download a whole region to use offline.

What is different here: there is no default map server and nothing is copied. You point it at any map source you choose, in the open PMTiles format. The program refuses to hammer public servers: it will not use the OpenStreetMap tile servers, it requires a clear User-Agent, it asks for few requests, and it writes the required attribution into every downloaded region. Places are stored as GeoJSON, an open standard.

## What it can do today

Latest tag: `v0.0.1`.

- Read map tiles from a remote PMTiles file (small range requests) or a local one.
- Cache tiles on disk with a hard size limit and a seven-day expiry.
- Download a region for offline use as a PMTiles package. It tells you the number of tiles and a size estimate first, has a tile limit, shows progress and can be cancelled.
- Keep places as GeoJSON: create, update, delete, find by name, and find everything inside an area, including areas that cross the antimeridian.
- Read and write `geo:` links (RFC 5870).
- Tested, including against a local range server and a real store.

## What is not there yet

- No window and no drawing of maps: tile decoding, styles, search by address, and routes are not started.
- No server, so other programs cannot reach it over a socket yet.
- No live notification when a place changes.
- The interface for places is a draft with no consumer yet.

## Part of FreeSynergy

Maps belongs to the groupware suite, next to people, calendar, wiki and messenger. FreeSynergy is a family of programs that are free for everyone, forever, and that keep your data yours. The documentation, the manifesto and the overall plan live in [fs-documentation](https://github.com/FreeSynergy/fs-documentation); the reading version is at <https://freesynergy.github.io/fs-documentation/>. Start with the [manifesto](https://freesynergy.github.io/fs-documentation/manifesto/) if you want to know why we do this.


## For developers

- Workspace crate: `fs-maps-core` (tile sources, cache, region download, places). Callers depend on the `TileSource` trait.
- The contract is `places.capnp` in [fs-interfaces](https://github.com/FreeSynergy/fs-interfaces) (draft). Maps owns it.
- Build and test: `cargo build --workspace` and `cargo test --workspace`. Examples: `fetch_tiles` and `places` (`cargo run -p fs-maps-core --example places`).
- Research behind the policy rules is in `RESEARCH.md`; design in [DESIGN.md](DESIGN.md).
- Licence: MIT OR Apache-2.0.
