# fs-maps - Design

Core (no GUI) and Cap'n Proto server of FreeSynergy maps: tiles load **piece by piece on demand**, are **cached locally with a size limit**,
regions can be **downloaded for offline use**, and any PMTiles source can be configured ("everybody loads their own map").
Evidence for all policy decisions: `RESEARCH.md` (2026-11-05). Decisions: ADR 0026 (newest standards), 0030 (originals),
0036 (packages via Git), 0013 (nothing copied: tiles are read from a source, not re-hosted by us).

## State

Version 0.2.0: `fs-maps-server`, the contract `places.capnp` over a Unix socket; `StorePlaces`, the owning provider with the live feed; `fs-interfaces` v0.22.0. Before, in 0.1: `Places` is the provider of `places.capnp` (ADR 0045, 0050): trait with every method, `Unsupported` defaults, `Capabilities`; `TileSource` also reports `Capabilities`. Earlier: ADR 0043 refactor done: no file over 300 lines (only `geouri.rs` is 212, parser plus its tests), `#![deny(missing_docs)]`, `clippy::pedantic` (a few noisy lints allowed in `lib.rs` with reasons), behaviour unchanged (30 tests before the provider work, all unchanged; 46 now).

| Area | State |
|---|---|
| `TileSource` trait (`id`, `get_tile(z,x,y) -> Result<Option<Bytes>>`, `info`) returning **uncompressed MVT** | done |
| `PmtilesHttpSource`: remote PMTiles via HTTP range requests (`pmtiles` crate, in-memory directory cache, max 4 requests in flight, 30 s timeout) | done, tested against a loopback range server |
| `PmtilesFileSource`: local PMTiles file | done |
| `FallbackSource` (region packages first, then cache+network), `MemorySource` | done |
| `TileCache`: disk, hard byte limit, LRU (recency persisted via mtime), 7-day TTL, atomic writes, rescan on open | done, model-based property test against a reference LRU |
| `CachedSource<S>` wrapper | done |
| `RegionDownload` (Facade; `DownloadCommand`, `TilePlan` iterator, `ProgressObserver`): exact tile count + sampled size estimate before download, tile limit (200 000), progress + cancel callback, writes a **PMTiles v3 package** (MVT, gzip, Hilbert order, bounds/metadata/attribution) via the `pmtiles` writer | done |
| `places`: `PlaceRepository` over `ObjectRepository` + GeoJSON `Feature` <-> `fs-store` `StoredObject` (type `Place`, `application/geo+json`, name from `properties.name`/`name:en`/`name:*`/`title`, index `geometry_type`, `bbox`, `geo`) | done, tested incl. real `TursoStore` |
| Trait `Places` (`places/contract.rs`): required `capabilities`, `get`, `list`; defaults answering `Unsupported`: `create`, `update`, `delete`, `within`, `find_by_name`. `PlaceRepository` implements all; it reports every id of the schema except `place.subscribe` (a borrowed repository has no live feed) | done |
| `StorePlaces` (`places/provider.rs`): owns an `Arc<dyn Store>`, so it can live behind `Arc<dyn Places>`; runs each call through a `PlaceRepository`; writes run one at a time (an update reads, then writes: a delete between the two was undone by it, reproduced in `tests/places_feed.rs`); reports all six ids of the schema | done |
| Trait `PlaceFeed` (`places/feed.rs`, Observer: `subscribe(listener)` -> `Subscription`; `Created` / `Updated` / `Deleted`), told after the commit, by `StorePlaces` only | done, draft |
| `PlaceRepository` over `ObjectRepository` + `ObjectQuery` (still borrows its store: `new(&store, actor)`; behind an `Arc<dyn Store>` use `StoreView::new(&*store)`); create sets context `places` and IRI `https://<instance>/places/<id>` (`with_instance`); update keeps id, `published`, IRI, attribution | done |
| `within(Area, Paging)`: **one store query**; the four edges of the geometry's bounding box are index keys (`lon_min`, `lon_max`, `lat_min`, `lat_max`: fixed-width decimals of 1e-7 degrees so the store's binary text order is numeric order), paged by the store. Overlap = bounding box of the geometry meets the area, edges touching count; points, lines, polygons alike; no geometry = never found | done, tested |
| Antimeridian: an area with `west > east` is `[west,180] + [-180,east]`; one OR-group (`lon_max >= west` or `lon_min <= east`), still one query. A geometry that itself crosses 180 has a bounding box over all longitudes (RFC 7946 §5), so it is found from both sides | done, tested |
| `find_by_name`: one store query, `Contains` (case-insensitive, normalised) on the envelope name or on the index `name` (every `name`, `name:*`, `title`) | done, tested |
| `TileSource::capabilities()` (provided: `tile.read`; `CachedSource` forwards its inner source) | done |
| `NoSource`: no map configured answers `Unsupported` for `tile.read`, not "no such tile"; `LazyHttpSource`: policy checked at once, the archive opened at the first tile and retried after a failure, so the program starts offline | done |
| Server `fs-maps-server`: every method of `Places` and `TileSource`, `subscribe` (with `ended`), `supportReport` on both; errors as typed problems (`places/not-found` 404, `places/conflict` 409, `places/invalid` 422, `places/usage-policy` 403, `places/tile-source-failed` 502, `places/store-failed` 500, `places/stored-data-invalid` 500, the shared `invalid-request` 422 and `too-large` 413, `Unsupported` 501); a read that fails ends the feed with `ended`; binary with the shared `--socket`, `--data`, `--actor` (data directory 0700), tiles from the environment: `FS_MAPS_TILES` (path list), `FS_MAPS_TILES_URL`, `FS_MAPS_USER_AGENT`, `FS_MAPS_CACHE`, `FS_MAPS_CACHE_MIB` | done, draft; open points below |
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
| `places/repository.rs` | Repository over Store | `PlaceRepository` depends on `fs_store::traits::ObjectRepository` only |
| `places/{geometry,name}.rs` | Interpreter-style recursion / Strategy | geometry validation; name fallback order |
| `tile/*`, `geouri.rs` | Newtype / Parser | validated `Tile`, `BBox`, `GeoUri` |
| `testutil/*` | Fake | loopback range server + PMTiles fixture |
| `places/feed.rs` | Observer | `PlaceFeed`, `PlaceListener`, `Registry` (subject), `Subscription` (drop = unsubscribe) |
| `places/provider.rs` | Facade + Observer | `StorePlaces`: what a program serves; one repository per call, the feed on top |
| `source/none.rs` | Null Object | a program without a map still has the interface, and says "not possible" |
| `source/lazy.rs` | Proxy | stands in for the HTTP source until the network answers |
| `fs-maps-server/root.rs` | Facade + Adapter | `MapsServer`, one capability for both interfaces, sent on by interface id; depends on the traits only |
| `fs-maps-server/{places_service,tiles_service}.rs` | Adapter | wire in, trait call, wire out |
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

`<dir>/<fnv1a64(source id) hex>/<z>/<x>/<y>.mvt` = 8-byte LE "stored at" + uncompressed MVT. Hard limit counts these file
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
* **Contract gap:** `places.capnp` has no `capabilities` method, unlike `tasks.capnp` and `history.capnp`, so `Capabilities::write_list` has nothing to answer; `supportReport` on both interfaces says the same and more. Proposal, append only: `capabilities @9` on `Places` and `@3` on `TileSource`.
* **Contract gap, provisional answer:** the schema has two root interfaces and no way from one to the other, a program has one socket. The root capability answers both by interface id; a consumer connects as `places::Client` or as `tile_source::Client` on the same socket, and any other interface is `unimplemented`. Proposal: `tiles @10 () -> (source :TileSource)` on `Places`, then this hand-written dispatch goes away.
* **Provisional:** `TileSource.id` of the `NoSource` answers `none`, not `Unsupported`: the method has no capability id of its own and "none" is true; `supportReport` and `tile` carry the "not possible".
* **Paging:** the server reads the whole result for each page, as `Page::of_weighted` needs it and the cursor's form is private to `fs_interfaces::rpc`; the store's own paging (`Paging { limit, offset }`) is unused on the wire, so a large `list` costs O(all places) per page. The limit is the shared one in `fs-interfaces/DESIGN.md`; with a public cursor position the server would need one line.
* **Provisional:** the tile configuration is read from the environment, because the shared `ServerOptions` refuses arguments it does not know and has no way to extend them.
* **Limits:** `check_intake` on the GeoJSON as it arrives and again as it will be stored (with room for the envelope), because `1E5` comes back as `100000.0`; so all that is stored can come back, worst case the whole text is the name, which the answer carries twice. A tile cannot be cut: above `MAX_PAGE_BYTES` it is `too-large`, not truncated, and `clamp_read` does not apply (the contract has no length). The pmtiles reader decompresses before that check: a hostile archive can still cost memory.
* **Stored data:** a `Place` in the store that no longer reads as a feature answers `places/stored-data-invalid` (500), and `list` fails on the page that holds it, until it is repaired. Only another writer of the store can leave one; skipping it silently would hide data. Question to the owner.
* **Checked in the core for the server:** the id of an object is never the client's (a feature's `id` member is content), so `create` cannot overwrite; tile and box values are validated before any arithmetic, tested with `u32::MAX` and zoom 255. Found and fixed: the read-modify-write race of `update` (`StorePlaces`). The order is the trait's: name, then id; the contract states none.
* **Feed:** it tells what went through `StorePlaces`, the only writer of the program's store. An undo through the store itself is not told; the contract has no undo. `PlaceRepository` alone has neither the feed nor the one-at-a-time rule. `Registry` has the same shape as in `fs-tasks-core` (ADR 0043 section 4: move it to a shared crate, a change for `fs-interfaces`).
* The server has no `--instance` yet, as in `fs-tasks-server`: IRIs say `local.invalid` (`StorePlaces::with_instance` exists).
* Tile ranges (`region`) still do not cross the antimeridian (`tile::BBox`, unchanged); the search `Area` is a separate type that does.
* Zoom > 22 and `crs` other than wgs84 are unsupported.
* Places: `Place` objects are plain GeoJSON features; AS2 `latitude/longitude/radius` are not mirrored (envelope has no
  free fields); JSON-FG is not handled yet (GeoJSON subset only). Geometry validation checks types and WGS84 ranges,
  not ring closure/winding.
* `rustls` brings a C/asm crypto provider (aws-lc-rs / ring): the stack is not 100 % pure Rust on the TLS side.

## Tests

`cargo test --workspace --all-features`. Core: tests by feature in `tests/` (`cache`, `cache_model`, `region`, `source`, `places`, `places_crud`, `places_find`, `places_feed`, `provider`, `http_fixture`), unit tests next to `tile`, `geouri`, `source/policy`; property tests (cache limit + LRU model, `geo:` roundtrip), integration test `tests/http_fixture.rs` (loopback range server, cache, region download, offline chain). Server: `tests/{places,find,errors,limits,report,feed,tiles,binary,refusals}.rs` over a real Unix socket against `TursoStore` behind `StorePlaces`, local PMTiles fixtures and the loopback range server; `binary` runs the real program across a restart. Nothing contacts a public server.
