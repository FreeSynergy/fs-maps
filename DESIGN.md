# fs-maps - Design

Core (no GUI) of FreeSynergy maps: tiles load **piece by piece on demand**, are **cached locally with a size limit**,
regions can be **downloaded for offline use**, and any PMTiles source can be configured ("everybody loads their own map").
Evidence for all policy decisions: `RESEARCH.md` (2026-11-05). Decisions: ADR 0026 (newest standards), 0030 (originals),
0036 (packages via Git), 0013 (nothing copied: tiles are read from a source, not re-hosted by us).

## State

Version 0.1.0 (local only, never pushed). ADR 0043 refactor done: no file over 300 lines (only `geouri.rs` is 212, parser plus its tests), `#![deny(missing_docs)]`, `clippy::pedantic` (a few noisy lints allowed in `lib.rs` with reasons), behaviour unchanged (30 tests: the 29 of before + 1 policy URL test).

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
| `geouri`: RFC 5870 `geo:` parse/format (crs=wgs84, `u=`, extra params, pole/antimeridian normalisation) | done |
| Example `fetch_tiles` | done (loopback fixture by default) |
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
* Antimeridian-crossing bboxes (split in two), zoom > 22, `crs` other than wgs84 are unsupported.
* Places: `Place` objects are plain GeoJSON features; AS2 `latitude/longitude/radius` are not mirrored (envelope has no
  free fields); JSON-FG is not handled yet (GeoJSON subset only). Geometry validation checks types and WGS84 ranges,
  not ring closure/winding.
* `rustls` brings a C/asm crypto provider (aws-lc-rs / ring): the stack is not 100 % pure Rust on the TLS side.

## Tests

`cargo test`: tests by feature in `tests/` (`cache`, `cache_model`, `region`, `source`, `places`, `http_fixture`), unit tests next to `tile`, `geouri`, `source/policy`; property tests (cache limit + LRU model, `geo:` roundtrip), integration test
`tests/http_fixture.rs` (loopback range server, cache, region download, offline chain). Nothing contacts a public server.
