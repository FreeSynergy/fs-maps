# fs-maps - Design

Core (no GUI) of FreeSynergy maps: tiles load **piece by piece on demand**, are **cached locally with a size limit**,
regions can be **downloaded for offline use**, and any PMTiles source can be configured ("everybody loads their own map").
Evidence for all policy decisions: `RESEARCH.md` (2026-11-05). Decisions: ADR 0026 (newest standards), 0030 (originals),
0036 (packages via Git), 0013 (nothing copied: tiles are read from a source, not re-hosted by us).

## State

Version 0.1.0 (first tag). `Places` is the provider of `places.capnp` (ADR 0045, 0050): trait with every method, `Unsupported` defaults, `Capabilities`; `TileSource` also reports `Capabilities`. Earlier: ADR 0043 refactor done: no file over 300 lines (only `geouri.rs` is 212, parser plus its tests), `#![deny(missing_docs)]`, `clippy::pedantic` (a few noisy lints allowed in `lib.rs` with reasons), behaviour unchanged (30 tests before the provider work, all unchanged; 46 now).

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
| Trait `Places` (`places/contract.rs`): required `capabilities`, `get`, `list`; defaults answering `Unsupported`: `create`, `update`, `delete`, `within`, `find_by_name`. `PlaceRepository` implements all; it reports every id of the schema except `place.subscribe` ("not built yet; it is possible") | done |
| `PlaceRepository` over `ObjectRepository` + `ObjectQuery` (still borrows its store: `new(&store, actor)`; behind an `Arc<dyn Store>` use `StoreView::new(&*store)`); create sets context `places` and IRI `https://<instance>/places/<id>` (`with_instance`); update keeps id, `published`, IRI, attribution | done |
| `within(Area, Paging)`: **one store query**; the four edges of the geometry's bounding box are index keys (`lon_min`, `lon_max`, `lat_min`, `lat_max`: fixed-width decimals of 1e-7 degrees so the store's binary text order is numeric order), paged by the store. Overlap = bounding box of the geometry meets the area, edges touching count; points, lines, polygons alike; no geometry = never found | done, tested |
| Antimeridian: an area with `west > east` is `[west,180] + [-180,east]`; one OR-group (`lon_max >= west` or `lon_min <= east`), still one query. A geometry that itself crosses 180 has a bounding box over all longitudes (RFC 7946 §5), so it is found from both sides | done, tested |
| `find_by_name`: one store query, `Contains` (case-insensitive, normalised) on the envelope name or on the index `name` (every `name`, `name:*`, `title`) | done, tested |
| `TileSource::capabilities()` (provided: `tile.read`; `CachedSource` forwards its inner source) | done |
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
* **Question to the owner:** `place.subscribe`, and the cursor form: the Rust API pages with `Paging { limit, offset }`; the wire `Cursor` will wrap the offset (offset paging costs O(offset) in the store).
* Tile ranges (`region`) still do not cross the antimeridian (`tile::BBox`, unchanged); the search `Area` is a separate type that does.
* Zoom > 22 and `crs` other than wgs84 are unsupported.
* Places: `Place` objects are plain GeoJSON features; AS2 `latitude/longitude/radius` are not mirrored (envelope has no
  free fields); JSON-FG is not handled yet (GeoJSON subset only). Geometry validation checks types and WGS84 ranges,
  not ring closure/winding.
* `rustls` brings a C/asm crypto provider (aws-lc-rs / ring): the stack is not 100 % pure Rust on the TLS side.

## Tests

`cargo test`: tests by feature in `tests/` (`cache`, `cache_model`, `region`, `source`, `places`, `places_crud`, `places_find`, `provider`, `http_fixture`), unit tests next to `tile`, `geouri`, `source/policy`; property tests (cache limit + LRU model, `geo:` roundtrip), integration test
`tests/http_fixture.rs` (loopback range server, cache, region download, offline chain). Nothing contacts a public server.
