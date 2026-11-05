# fs-maps research (2026-11-05)

Legend: ✅ verified on the page/API today, ⚠️ not verified / interpretation.

## 1. OSMF tile usage policy (https://operations.osmfoundation.org/policies/tiles/)

| Topic | Finding |
|---|---|
| Scope | ✅ Only the raster tiles `https://tile.openstreetmap.org/{z}/{x}/{y}.png` (Standard layer). Vector tiles at vector.openstreetmap.org have a separate policy (https://operations.osmfoundation.org/policies/vector/ ⚠️ not read). |
| Bulk / prefetch | ✅ **Prohibited**: pre-seeding areas or zoom stacks, building tile archives (.zip, .mbtiles), scans over bounding boxes, headless bots. |
| Offline use | ✅ **Not permitted** on tile.openstreetmap.org; "Download city for offline use" is explicitly named as prohibited. The policy itself points to vector tiles / self-hosting for offline use. |
| Interactive use | ✅ Allowed: tiles of the current viewport (modest look-ahead). |
| User-Agent | ✅ Required, clear and stable, naming the app (+ contact). Library defaults (`reqwest`, `curl`, ...) are blocked. |
| Caching | ✅ Honour `Cache-Control`/`Expires`/`ETag`; if the cache cannot read headers, keep tiles **>= 7 days**; use conditional requests (`If-None-Match`, `If-Modified-Since`); never send `no-cache` by default. |
| Attribution | ✅ Visible "© OpenStreetMap contributors" on the map. |
| Other | ✅ HTTPS only; no SLA; may be blocked without notice; the tile URL should be configurable (switchable without an update). |

**Consequence:** the public OSM raster server can never be the source for our "download a region" feature and must not be a default.
It is only usable for live viewing, and our app is built around cache + prefetch + offline. We do not use it at all.

## 2. Nominatim usage policy (https://operations.osmfoundation.org/policies/nominatim/)

- ✅ Max 1 request/second absolute (per application, summed over all users); valid Referer or own User-Agent.
- ✅ Only end-user-triggered requests; **no autocomplete**, no systematic/grid queries, no scraping of the details page, no periodic requests from apps.
- ✅ Results must be cached; attribution (ODbL) required; the service must be switchable without a software update.
- ✅ Not allowed to be built in as a generic geocoder in no-code/low-code platforms; primary-geocoding products must run their own.
- Consequence: geocoding is **not part of this core**. Later: configurable endpoint, user-triggered search only, 1 req/s limiter, cache. Self-hosted Nominatim / Photon is the real answer for FreeSynergy.

## 3. PMTiles v3

- ✅ Spec (https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md): single file = 127-byte header, root directory (must lie within the first 16 KiB), JSON metadata, optional leaf directories, tile data.
  Header: magic `PMTiles`, version 3, offsets/lengths (u64 LE), tile/entry/content counts, clustered flag, internal compression and tile compression (none/gzip/brotli/zstd), tile type (1 = MVT, 2 = PNG, ...), min/max zoom, bounds and centre (i32 LE x 1e7).
  Directory entries: TileID (Hilbert curve over all zoom levels), offset, length, run length (0 = leaf directory); varint, delta-encoded, compressed with the internal compression.
- ✅ Access over HTTP = `Range` requests: first request bytes 0-16383 (header + root dir), then one request per leaf directory (cacheable) and one per tile.
- ✅ Licence: spec public domain / CC0, reference implementations BSD-3-Clause (https://github.com/protomaps/PMTiles README, LICENSE).
- ✅ Rust crate `pmtiles` 0.27.0 (https://crates.io/crates/pmtiles, repo stadiamaps/pmtiles-rs): **MIT OR Apache-2.0**, edition 2024, MSRV 1.88.
  Reads and **writes** v3 (`write` feature, `PmTilesWriter`, identical tiles stored once); backends: local file (`file-async-tokio`, default), mmap, HTTP via reqwest (`http-async` + `reqwest-rustls`), S3, `object_store`; directory caches (`HashMapCache`, `MokaCache`); sync reader; fuzzed against untrusted input; builds for WASM.
  Pure Rust core; gzip via `flate2` (zlib-rs, pure Rust). `zstd` feature wraps C `libzstd` (`zstd` crate), `brotli` is pure Rust. `reqwest-rustls` pulls rustls with a crypto provider (aws-lc-rs or ring, both contain C/asm) - acceptable, but not "100 % pure Rust" on the TLS side. The writer cannot write zstd/brotli without those features; we write **gzip** tiles (MVT is typically gzip).
- Decision: use the `pmtiles` crate (reader + writer + HTTP); no own parser.

## 4. Free OSM-based PMTiles sources

| Source | Finding |
|---|---|
| Protomaps daily builds (https://docs.protomaps.com/basemaps/downloads) | ✅ Planet basemap, one PMTiles file, z0-15, ~120 GB; "ODbL Produced Work, OpenStreetMap attribution required"; daily builds at `maps.protomaps.com/builds`; generation is open source (github.com/protomaps/basemaps). ✅ **"Hotlinking to these downloads is discouraged; copy the tileset to your own cloud storage."** Builds are retained for a week (+ latest per patch version), so URLs are not stable. ⚠️ No explicit permission to use the build bucket as a production tile server was found. |
| Region cut-outs | ✅ `pmtiles extract SRC.pmtiles OUT.pmtiles --bbox=minlon,minlat,maxlon,maxlat [--maxzoom N] [--region file.geojson]` (Go CLI, https://github.com/protomaps/go-pmtiles, BSD-3-Clause); source may be remote, must be clustered; a full pyramid 0..maxzoom needs minimal requests; each extra zoom level roughly doubles the size. There is **no Rust equivalent CLI**; our `RegionDownload` re-implements the idea (tile list for bbox -> tile reads -> `PmTilesWriter`). |
| Geofabrik (https://download.geofabrik.de/) | ✅ Free *OSM data* extracts (`.osm.pbf`, daily, personal data stripped), not tiles. Basis for building own tiles with planetiler/tilemaker. Shortbread tiles (Baden-Wuerttemberg, 360 MB) are offered on shortbread-tiles.org as an example. |
| Own hosting (recommended) | Put a Protomaps/Shortbread PMTiles file on any static server with range support (also on GitHub release assets/LFS, or Git per ADR 0036 for small regional packages). Users can add any PMTiles URL or file ("everybody loads their own map"). |

Schemas (what the vector tiles contain):
- ✅ Protomaps basemap v4 layers (https://docs.protomaps.com/basemaps/layers): `boundaries, buildings, earth, landcover, landuse, natural, places, pois, roads, transit, water`; derived from OSM + Natural Earth; schema derived from Tilezen. Data under ODbL (Produced Work); build code BSD-3 (⚠️ tile-schema licence not separately stated).
- ✅ Shortbread (https://shortbread-tiles.org/): lean community schema, documentation CC0; tile data ODbL.
- ✅ OpenMapTiles schema: code BSD-3, **design licence CC-BY 4.0 - requires visible credit "OpenMapTiles"** (https://github.com/openmaptiles/openmaptiles/blob/master/LICENSE.md). Avoid as default for that reason.
- Attribution to show in any map UI: "© OpenStreetMap contributors" (ODbL), plus "Protomaps" if its builds are used.

## 5. Rust MVT decoding / rendering (for the GUI later)

| Crate | Version | Licence | Note |
|---|---|---|---|
| `mvt-reader` | 2.5.0 | MIT | ✅ pure Rust decode of MVT (also WASM) |
| `geozero` | 0.15.1 | MIT OR Apache-2.0 | ✅ MVT <-> GeoJSON/WKT/... conversions |
| `geo-types` / `geojson` | 0.7.20 / 1.0.0 | MIT OR Apache-2.0 | ✅ |
| `lyon` | 1.0.19 | MIT OR Apache-2.0 | ✅ pure Rust path tessellation -> triangles for the GPU (Blinc) |
| `tilejson` | 0.4.4 | MIT OR Apache-2.0 | ✅ TileJSON |

Plan: decode with `mvt-reader`, style (own, small), tessellate with `lyon`, draw through Blinc. Labels/glyphs are the hard part (not studied).
This core stores **raw MVT bytes** so the renderer is independent.

## 6. Recommendation

1. Default: **no hard-coded server**. The map app ships with a configurable list of PMTiles sources (URL or file). Suggested default for FreeSynergy: a PMTiles file we host ourselves (extract of the Protomaps/Shortbread build for the user's region), not the Protomaps build bucket.
2. Never use tile.openstreetmap.org for offline/prefetch (forbidden); not used at all.
3. Remote reads = range requests with our own User-Agent; region download = reading only the needed tile ranges of a PMTiles source and writing a local PMTiles file (offline package).
