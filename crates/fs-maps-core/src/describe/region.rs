//! `Region`: an area downloaded for offline use, a PMTiles package.
//!
//! Pattern: Registry entry (`DescribesType`). Standard: **PMTiles v3** for the package (the core writes it, it works with
//! any PMTiles tool), the area as two `geo:` URIs (RFC 5870, south-west and north-east corner). The core has the
//! download (`RegionDownload`), but `places.capnp` has no call for it: every command is unsupported until it has.

use fs_types::{BasicType, CommandKind, DescribesType, TypeDescription};

use super::{command, field, find, record};

/// Marker for the description of an offline region.
pub struct RegionType;

const T: &str = "region";

impl DescribesType for RegionType {
    fn describe() -> TypeDescription {
        let area = |c: fs_types::CommandDescription| {
            c.input(field(T, "southwest", BasicType::Place).required())
                .input(field(T, "northeast", BasicType::Place).required())
                .input(field(T, "minZoom", BasicType::Text).required())
                .input(field(T, "maxZoom", BasicType::Text).required())
        };
        TypeDescription::new(
            "Region",
            "fs-maps",
            "PMTiles v3 package; geo: URI (RFC 5870)",
            T,
        )
        .field(field(T, "name", BasicType::Text).required())
        .field(field(T, "southwest", BasicType::Place).required())
        .field(field(T, "northeast", BasicType::Place).required())
        .field(field(T, "minZoom", BasicType::Text).required())
        .field(field(T, "maxZoom", BasicType::Text).required())
        .command(find(T))
        // The exact tile count and a size estimate before the download.
        .command(
            area(command(T, "estimate", CommandKind::Other))
                .result(field(T, "tiles", BasicType::Text))
                .result(field(T, "size", BasicType::Text)),
        )
        .command(
            area(
                command(T, "download", CommandKind::Create)
                    .input(field(T, "name", BasicType::Text).required()),
            )
            .result(field(T, "id", BasicType::Reference).target("Region")),
        )
        .command(command(T, "delete", CommandKind::Delete).input(record(T, "Region")))
    }
}
