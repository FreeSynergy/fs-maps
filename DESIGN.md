# fs-maps - Design

Core (no GUI) and Cap'n Proto server of FreeSynergy maps: tiles load **piece by piece on demand**, are **cached locally with a size limit**,
regions can be **downloaded for offline use**, and any PMTiles source can be configured ("everybody loads their own map").
Evidence for all policy decisions: `RESEARCH.md` (2026-11-05). Decisions: ADR 0026 (newest standards), 0030 (originals),
0036 (packages via Git), 0013 (nothing copied: tiles are read from a source, not re-hosted by us).

## State

Version 0.3.0: `fs-maps-server`, the contract `places.capnp` over a Unix socket; `StorePlaces`, the owning provider with the live feed; `fs-interfaces` v0.45.0. Before, in 0.1: `Places` is the provider of `places.capnp` (ADR 0045, 0050): trait with every method, `Unsupported` defaults, `Capabilities`; `TileSource` also reports `Capabilities`. Earlier: ADR 0043 refactor done: no file over 300 lines (only `geouri.rs` is 212, parser plus its tests), `#![deny(missing_docs)]`, `clippy::pedantic` (a few noisy lints allowed in `lib.rs` with reasons), behaviour unchanged (30 tests before the provider work, all unchanged; 46 now).

| Area | State |
|---|---|
| `TileSource` trait (`id`, `get_tile(z,x,y) -> Result<Option<Bytes>>`, `info`) returning **uncompressed MVT** | done |
| `PmtilesHttpSource`: remote PMTiles via HTTP range requests (`pmtiles` crate, in-memory directory cache, max 4 requests in flight, 30 s timeout) | done, tested against a loopback range server |
| `PmtilesFileSource`: local PMTiles file | done |
| `FallbackSource` (region packages first, then cache+network), `MemorySource` | done |
| `TileCache`: disk, hard byte limit, LRU (recency persisted via mtime), 7-day TTL, atomic writes, rescan on open | done, model-based property test against a reference LRU |
| `CachedSource<S>` wrapper | done |
| `RegionDownload` (Facade; `DownloadCommand`, `TilePlan` iterator, `ProgressObserver`): exact tile count + sampled size estimate before download, tile limit (200 000), progress + cancel callback, writes a **PMTiles v3 package** (MVT, gzip, Hilbert order, bounds/metadata/attribution) via the `pmtiles` writer | done |
| `places`: `PlaceRepository` over `ObjectRepository` + GeoJSON `Feature` <-> `fs-objects` `StoredObject` (type `Place`, `application/geo+json`, name from `properties.name`/`name:en`/`name:*`/`title`, index `geometry_type`, `bbox`, `geo`) | done, tested incl. real `TursoStore` |
| Trait `Places` (`places/contract.rs`): required `capabilities`, `get`, `list`; defaults answering `Unsupported`: `create`, `update`, `delete`, `within`, `find_by_name`. `PlaceRepository` implements all; it reports every id of the schema except `place.subscribe` (a borrowed repository has no live feed) | done |
| `StorePlaces` (`places/provider.rs`): owns an `Arc<dyn Store>`, so it can live behind `Arc<dyn Places>`; runs each call through a `PlaceRepository`; writes run one at a time (an update reads, then writes: a delete between the two was undone by it, reproduced in `tests/places_feed.rs`); reports all six ids of the schema | done |
| Trait `PlaceFeed` (`places/feed.rs`, Observer: `subscribe(listener)` -> `Subscription`; `Created` / `Updated` / `Deleted`), told after the commit, by `StorePlaces` only | done, draft |
| `PlaceRepository` over `ObjectRepository` + `ObjectQuery` (still borrows its store: `new(&store, actor)`; behind an `Arc<dyn Store>` use `StoreView::new(&*store)`); create sets context `places` and IRI `https://<instance>/places/<id>` (`with_instance`); update keeps id, `published`, IRI, attribution | done |
| `within(Area, Paging)`: **one store query**; the four edges of the geometry's bounding box are index keys (`lon_min`, `lon_max`, `lat_min`, `lat_max`: fixed-width decimals of 1e-7 degrees so the store's binary text order is numeric order), paged by the store. Overlap = bounding box of the geometry meets the area, edges touching count; points, lines, polygons alike; no geometry = never found | done, tested |
| Antimeridian: an area with `west > east` is `[west,180] + [-180,east]`; one OR-group (`lon_max >= west` or `lon_min <= east`), still one query. A geometry that itself crosses 180 has a bounding box over all longitudes (RFC 7946 §5), so it is found from both sides | done, tested |
| `find_by_name`: one store query, `Contains` (case-insensitive, normalised) on the envelope name or on the index `name` (every `name`, `name:*`, `title`) | done, tested |
| `TileSource::capabilities()` (provided: `tile.read`; `CachedSource` forwards its inner source) | done |
| `NoSource`: no map configured answers `Unsupported` for `tile.read`, not "no such tile"; `LazyHttpSource`: policy and client checked at once, the archive opened at the first tile, a failed open remembered for 30 s (8 s to open, 5 s to connect), opened again when its `ETag` changed, so the program starts offline | done |
| At open: only MVT in gzip, brotli or no compression; a bad local file refuses the start | done |
| Cache: keyed by archive version, the last version remembered for an offline restart; an expired tile stays until a fetch replaced it and answers while the source is away; 0700/0600; stray files skipped | done |
| Server `fs-maps-server`: every method of `Places` and `TileSource`, `subscribe` (with `ended`), `supportReport` on both; errors as typed problems (`places/not-found` 404, `places/conflict` 409, `places/invalid` 422, `places/usage-policy` 403, `places/tile-source-failed` 502, `places/store-failed` 500, `places/stored-data-invalid` 500, the shared `invalid-request` 422 and `too-large` 413, `Unsupported` 501); a read that fails ends the feed with `ended`; binary with the shared `--socket`, `--data`, `--actor`, `--instance` (data directory 0700), tiles from the environment: `FS_MAPS_TILES` (path list), `FS_MAPS_TILES_URL`, `FS_MAPS_USER_AGENT`, `FS_MAPS_CACHE` (0700, default below `$XDG_CACHE_HOME/freesynergy/maps`, one per data directory), `FS_MAPS_CACHE_MIB` | done, draft; open points below |
| `geouri`: RFC 5870 `geo:` parse/format (crs=wgs84, `u=`, extra params, pole/antimeridian normalisation) | done |
| Examples `fetch_tiles` (loopback fixture by default), `places` | done |
| GUI, MVT decoding/rendering, styles, geocoding/search, routing | **not started** (see RESEARCH.md section 5) |

## Structure and pattern map (ADR 0043)

Every file is one concept, <= 200 lines (tests included; tests live in `tests/<feature>.rs`), `lib.rs`/`mod.rs` only
declare and re-export, every module doc has a `Pattern:` line. Callers depend on the `TileSource` trait.

| Module | Pattern | Why |
|---|---|---|
| `source/tile_source.rs` | Strategy | `TileSource` is the one interface for "where tiles come from" |
| `source/{file,http,pmt}.rs` | Adapter | the `pmtiles` reader (file / HTTP range) behind `TileSource` |
| `source/policy.rs` | Strategy (validation rules) | OSMF host ban, scheme, User-Agent checked before any request |
| `source/memory.rs` | Strategy implementation | simplest source, tests and tiny maps |
| `source/fallback.rs` | Chain of Responsibility | region packages first, then cache + network; first hit wins |
| `cache/cached.rs` | Decorator | `CachedSource<S>` is a `TileSource` wrapping another one |
| `cache/{tile_cache,maintenance}.rs` | Repository / Strategy (LRU) | get/put/clear by key; eviction policy in its own file |
| `cache/index.rs`, `cache/disk.rs` | Repository (internal) / Adapter | in-memory LRU index; file layout and atomic writes |
| `cache/{config,stats}.rs` | Value objects | plain data |
| `region/download.rs` | Facade | `RegionDownload`: estimate + download behind two calls |
| `region/command.rs` | Command | `DownloadCommand::run` carries source, region, plan, target; cleans up on error/cancel |
| `region/plan.rs` | Iterator | tiles in Hilbert (TileID) order, as the clustered PMTiles archive needs |
| `region/observer.rs` | Observer | `ProgressObserver` (closures work too); returning `false` cancels |
| `region/estimate.rs` | Strategy consumer | pure function over a plan and a `dyn TileSource` |
| `places/contract.rs` | Repository (interface) + Null Object | `Places`: what callers depend on; `Unsupported` defaults |
| `places/{write,search,book}.rs` | Template method / Builder consumer / Facade | write flow; one `Query` per search; thin trait impl + capability report |
| `places/bounds.rs` | Specification | `Area` turns itself into store conditions; edge keys of a box |
| `places/store_view.rs` | Adapter | `&dyn Store` as the two narrow traits |
| `places/adapter.rs` | Adapter | GeoJSON `Feature` <-> `StoredObject` (type `Place`) |
| `places/repository.rs` | Repository over Store | `PlaceRepository` depends on `fs_objects::traits::ObjectRepository` only |
| `places/{geometry,name}.rs` | Interpreter-style recursion / Strategy | geometry validation; name fallback order |
| `tile/*`, `geouri.rs` | Newtype / Parser | validated `Tile`, `BBox`, `GeoUri` |
| `testutil/*` | Fake | loopback range server + PMTiles fixture |
| `places/feed.rs` | Observer | `PlaceFeed`, `PlaceListener`, `Registry` (subject), `Subscription` (drop = unsubscribe) |
| `places/provider.rs` | Facade + Observer | `StorePlaces`: what a program serves; one repository per call, the feed on top |
| `source/none.rs` | Null Object | a program without a map still has the interface, and says "not possible" |
| `source/lazy.rs` | Proxy | stands in for the HTTP source until the network answers |
| `source/guard.rs` | Decorator | limits in front of what the `pmtiles` reader reads |
| `source/decode.rs` | Strategy + Guard | decompressing, never further than the limit |
| `source/directories.rs` | Cache (FIFO) | leaf directories within a budget |
| `source/http_backend.rs` | Adapter | range requests within the policy; no URL in errors |
| `cache/versions.rs` | Repository (internal) | the last version seen; tiles kept past their time |
| `fs-maps-server/places_service.rs` | Adapter + Factory | `MapsServer`, the root; `tiles` hands out the other capability; depends on the traits only |
| `fs-maps-server/tiles_service.rs` | Adapter | wire in, `TileSource` call, wire out |
| `fs-maps-server/{wire,errors,feed}.rs` | Adapter, Observer | places on the wire and what fits a message, core errors as RFC 9457 problems, `PlaceFeed` to the shared forwarder |
| `fs-maps-server/sources.rs` | Factory + Chain of Responsibility | which tiles, from the configuration: local packages first, then the remote archive through the cache |

## Policy rules built in

* No default server. Sources are configured (URL or file). `tile.openstreetmap.org` (+ subdomains) is **refused** by
  `PmtilesHttpSource` (OSMF forbids offline/prefetch there; it is raster PNG, not PMTiles anyway).
* A clear User-Agent is mandatory for HTTP sources; generic library defaults (`reqwest`, `curl`, ...) are rejected.
* Requests are few: header + root directory once, leaf directories cached in memory, then exactly one range request per
  tile, never the whole file; region download is sequential (one request at a time).
* Cache default TTL 7 days (OSMF minimum for tile caches); PMTiles content is immutable per archive version, the
  `pmtiles` reader fails if the archive changed underneath (ETag/Last-Modified check).
* Attribution "© OpenStreetMap contributors" is written into region packages' metadata and must be shown by every UI.

## Region package format

A normal PMTiles v3 file (so it works with any PMTiles tool and can be distributed via Git/LFS or file share):
MVT tiles gzip-compressed, clustered, header bounds = region bbox, metadata JSON `{name, attribution, source, bounds,
minzoom, maxzoom, generator}`. Written to `<name>.pmtiles.part`, renamed on success; cancel/error removes the partial file.
Note the `pmtiles` writer reserves 16 KiB for header + root directory, so tiny packages are never smaller than that.

## Cache format

`<dir>/<fnv1a64(source key + "#" + archive version) hex>/<z>/<x>/<y>.mvt` = 8-byte LE "stored at" + uncompressed MVT; `<dir>/versions/<fnv1a64(source key) hex>` holds the last version seen. For this user alone (0700, 0600). Hard limit counts these file
sizes. A single tile larger than the limit is not stored. Eviction removes the least recently used entry (hits touch the
file mtime so the order survives restarts). One process owns a cache dir (no cross-process locking).

## Open points

* Region download fetches tile by tile. The Go `pmtiles extract` merges neighbouring byte ranges (`--overfetch`) and uses
  parallel downloads; for large regions a range-merging extractor on the source's directory entries is the next step
  (the crate offers `entries()` via the `iter` feature).
* Estimate counts the bbox pyramid, not the tiles that really exist (ocean / empty areas are missing tiles; sample average
  includes missing ones as 0 bytes, so it is a rough guess).
* The cache stores uncompressed MVT (simple for the renderer); gzip on disk would save roughly 60 % - decide when the
  renderer exists.
* Which PMTiles source FreeSynergy hosts by default (recommendation: own extract of a Protomaps/Shortbread build, ODbL
  attribution). Protomaps asks not to hotlink their daily builds; no explicit permission to use that bucket as a tile
  server was found.
* **Question to the owner (provisional):** the overlap rule. Today a place matches `within` when the bounding box of its geometry meets the area (an L-shaped polygon matches a box in its empty notch; a geometry over the antimeridian matches everywhere along the latitude band). Exact geometry tests would need a geometry library (ADR 0015) and could not run in the store.
* **Question to the owner (provisional):** index precision 1e-7 degrees (about 1 cm), rounded outwards, so a place at most 1e-7 degrees outside an area can still match. Raising it would need a wider key.
* **Decided by the strategy:** `supportReport` is the standard report of this contract; `capabilities` is not added to `places.capnp`.
* The consumer reaches tiles through `Places.tiles` (`fs-interfaces`): one socket, one root capability.
* **Provisional:** `TileSource.id` of the `NoSource` answers `none`, not `Unsupported`: the method has no capability id of its own and "none" is true; `supportReport` and `tile` carry the "not possible".
* **Paging:** the server reads the whole result for each page, as `Page::of_weighted` needs it and the cursor's form is private to `fs_interfaces::rpc`; the store's own paging (`Paging { limit, offset }`) is unused on the wire, so a large `list` costs O(all places) per page. The limit is the shared one in `fs-interfaces/DESIGN.md`; with a public cursor position the server would need one line.
* **Provisional:** the tile configuration is read from the environment, because the shared `ServerOptions` refuses arguments it does not know and has no way to extend them.
* **Zstd is refused at open with a clear reason:** the `zstd` crate binds the C library, against the 100 % Rust rule. Gzip and brotli are read; brotli and `zlib-rs` are Rust. A pure-Rust decoder (`ruzstd`) could read zstd tiles if real archives need it, directories would then need more than the reader offers.
* **An archive without `ETag` and `Last-Modified` has no version:** the cache cannot tell its versions apart, and the reader cannot notice a replacement.
* **Limits:** `check_intake` on the GeoJSON as it arrives and again as it will be stored (with room for the envelope), because `1E5` comes back as `100000.0`; so all that is stored can come back, worst case the whole text is the name, which the answer carries twice. A tile cannot be cut: above `MAX_PAGE_BYTES` it is `too-large`, not truncated, and `clamp_read` does not apply (the contract has no length).
* **Hostile archives:** nothing is read or decompressed past a limit. `source/guard.rs` refuses a read over `MAX_TILE_BYTES` before it happens and tries each directory up to `MAX_DIRECTORY_BYTES` (4 MiB) before the reader decompresses it; tiles are decompressed in `source/decode.rs`; leaf directories share a 64 MiB budget; the HTTP backend stops at the bytes it asked for; each archive reads four tiles at a time, also from a local file. Measured on the release binary (`tests/memory.rs`, `VmHWM`): a tile bomb with 64 calls 155 MiB, a directory at the limit with 256 calls 155 MiB, one over it 40 MiB.
* **Allowed tiles (provisional, Thread 3):** `MAX_TILE_BYTES` is 8 MiB. Vector tiles are normally far below 1 MiB, so this refuses only unusual archives, with a clear error on those tiles. Answers wait in the connection's write queue, which has no back pressure, so at most about 8 MiB times the calls at once stay in memory; 64 calls of the former 32 MiB limit reached 2104 MiB. A true bound needs flow control on the rpc side.
* **Provisional, reckoned and not measured on a real planet:** `MAX_DIRECTORY_BYTES`. The specification keeps the root under 16 KiB, about 2 500 compressed entries, and a writer grows its leaves until it fits; some 350 million entries (an assumed planet, no archive was read) then make leaves of about 150 000 entries at 6 to 10 bytes, 1 to 2 MiB. An archive beyond 4 MiB gets a clear error on those tiles.
* **A local file replaced while running** answers failures until a restart; only the remote source reopens.
* **The usage policy holds through redirects:** only the same host and port, except http on 80 to https on 443, never from https to http, every target through `check_url`; no `Referer`; a trailing dot does not make another host.
* **A user and password in the URL are refused at the start:** they would never be sent, so an archive behind Basic auth could only fail. A token in the query is supported: `id`, `supportReport` and error texts show the URL without it (`public_id`, `without_url`); the cache keys by all of it, hashed.
* **An archive that changes:** a read of another version is `SourceModified` before anything reads its bytes, and a 416 is too; the remote source reopens once per tile. A tile is cached only under the version it was asked under, so one read under way during a change is served and not kept.
* **Writes run to their end** (`run_to_end`): a client that goes away does not stop one between its commit and telling the listeners, and a program that stops waits for it.
* **A place no message can carry** (only another writer of the store can leave one) answers `too-large` with a cursor past it (`Problem::next`), so `list` stays readable behind it.
* **Rules added later do not take stored places away:** the shape of a geometry is checked on the way in, only positions on the way out.
* **At the edges of the world (`within`):** an area ending at 180 or -180 also finds what touches the other one. A geometry that is only a pole is found at every longitude. A geometry that merely reaches a pole keeps its own longitudes, as its bounding box says; so does a `MultiPoint` at both poles.
* **Stored data:** a `Place` in the store that no longer reads as a feature answers `places/stored-data-invalid` (500), and `list` fails on the page that holds it, until it is repaired. Only another writer of the store can leave one; skipping it silently would hide data. Question to the owner.
* **Checked in the core for the server:** the id of an object is never the client's (a feature's `id` member is content), so `create` cannot overwrite; tile and box values are validated before any arithmetic, tested with `u32::MAX` and zoom 255. Found and fixed: the read-modify-write race of `update` (`StorePlaces`). The order is the trait's: name, then id; the contract states none.
* **Feed:** it tells what went through `StorePlaces`, the only writer of the program's store. An undo through the store itself is not told; the contract has no undo. `PlaceRepository` alone has neither the feed nor the one-at-a-time rule. `Registry` has the same shape as in `fs-tasks-core` (ADR 0043 section 4: move it to a shared crate, a change for `fs-interfaces`).
* Tile ranges (`region`) still do not cross the antimeridian (`tile::BBox`, unchanged); the search `Area` is a separate type that does.
* Zoom > 22 and `crs` other than wgs84 are unsupported.
* Places: `Place` objects are plain GeoJSON features; AS2 `latitude/longitude/radius` are not mirrored (envelope has no
  free fields); JSON-FG is not handled yet (GeoJSON subset only). Geometry validation checks types and WGS84 ranges,
  not ring closure/winding.
* `rustls` brings a C/asm crypto provider (aws-lc-rs / ring): the stack is not 100 % pure Rust on the TLS side.

## Tests

`cargo test --workspace --all-features`. Core: tests by feature in `tests/` (`cache`, `cache_stale`, `cache_model`, `region`, `source`, `remote`, `remote_policy`, `hostile`, `places`, `places_crud`, `places_find`, `places_edges`, `places_feed`, `provider`, `http_fixture`), unit tests next to `tile`, `geouri`, `source/policy`; property tests (cache limit + LRU model, `geo:` roundtrip), integration test `tests/http_fixture.rs` (loopback range server, cache, region download, offline chain). Server: `tests/{places,find,errors,limits,report,feed,tiles,tiles_remote,binary,binary_cache,refusals}.rs` over a real Unix socket against `TursoStore` behind `StorePlaces`, local PMTiles fixtures and the loopback range server; `binary` runs the real program across a restart. Nothing contacts a public server.

## One binary `fs-maps` and its window (ADR 0063, 2026-11-08)

`crates/fs-maps` is the only binary: `fs-maps` without arguments opens the window, `fs-maps serve` runs the server (`fs_maps_server::serve`; tiles from `FS_MAPS_TILES` / `FS_MAPS_TILES_URL`), feature `gui` (default) builds the window. `fs-maps-server` and `fs-maps-app` are libraries. The window (`crates/fs-maps-app`) is a frontend over `places.capnp`: `PlacesApi` (trait first) with `PlacesRemote` (a worker thread owns the Cap'n Proto connection) and `PlacesAdapter`, which gives the places to the list-and-page workbench of `fs-components` (built for people, neutral in what it shows) as rows (name, position), a sheet (position, geo URI, description) and a form (name, latitude, longitude, description). A place that is not a single point is renamed and described, its shape is kept as it is. If no server answers on the user's own socket the window starts `fs-maps serve` detached. Tests: `crates/fs-maps-app/tests/remote.rs` (real server, real store), `crates/fs-maps/tests/binary*.rs`, `memory.rs`, `refusals.rs` (the real binary).

**There is no map yet:** no tile view, no pan or zoom, no place search by area (`within`), no tile download of a region; tiles are served by the server (`tiles.capnp`) and nobody draws them. That view needs its own widget in `fs-components` (raster or vector tiles, pointer and wheel events) and is the next piece. A bad latitude or longitude is refused with a sentence on Save, not while typing.

## Unsupported

Commands the type descriptions (`crates/fs-maps-core/src/describe/`) list but the core does not carry out. The table is `SUPPORT` in `describe/support.rs` (Specification; a test keeps it and this list in step). `types.capnp` has no field `supported` in `CommandDescription` yet (Thread T adds it); until then the table stands in, and the window answers these commands with `BackendError::Unavailable("unsupported:<name>")`. Switch one on in the table only.

* `Place.elevationProfile`: elevation profile along a drawn line (Google Earth); no elevation data in the core.
* `MapLayer.find`, `MapLayer.create`, `MapLayer.change`, `MapLayer.delete`: the core has no layers.
* `MapLayer.refresh`: layers connected to a data source that stay current (Felt; WFS, WMTS, GeoJSON).
* `MapLayer.invite`: several people edit one map live (Felt, uMap).
* `MapLayer.comment`: comment on a map without an editing seat (Felt; ActivityStreams Note).
* `MapLayer.webhook`: map events and webhooks for embedding (Felt).
* `MapTour.find`, `MapTour.create`, `MapTour.change`, `MapTour.delete`: maps with a guided tour or story (ordered stops); the core has no tours.
* `Region.find`, `Region.estimate`, `Region.download`, `Region.delete`: the core has `RegionDownload` (estimate, download, PMTiles package), but `places.capnp` has no call for it.
