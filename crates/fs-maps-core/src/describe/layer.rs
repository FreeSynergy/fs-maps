//! `MapLayer`: a set of places drawn together, from the program's own places or from a source that stays current.
//!
//! Pattern: Registry entry (`DescribesType`). Standard: **GeoJSON FeatureCollection (RFC 7946)** for the content; a
//! layer that follows a source names a WFS or WMTS service (OGC) or a PMTiles archive. Chosen because the optional
//! requirements name exactly these (GeoJSON, WFS, WMTS). The core has no layers yet: every command is unsupported.

use fs_types::{BasicType, CommandKind, DescribesType, TypeDescription};

use super::{command, field, find, record};

/// Marker for the description of a map layer.
pub struct MapLayerType;

const T: &str = "layer";

impl DescribesType for MapLayerType {
    fn describe() -> TypeDescription {
        let own = |c: fs_types::CommandDescription, name_required: bool| {
            let name = field(T, "name", BasicType::Text);
            c.input(if name_required { name.required() } else { name })
                .input(field(T, "description", BasicType::MultilineText))
                .input(
                    field(T, "source", BasicType::Choice)
                        .choices(&["places", "geojson", "wfs", "wmts", "pmtiles"]),
                )
                .input(field(T, "url", BasicType::Link))
                .input(
                    field(T, "places", BasicType::Reference)
                        .target("Place")
                        .multiple(),
                )
        };
        TypeDescription::new(
            "MapLayer",
            "fs-maps",
            "GeoJSON FeatureCollection (RFC 7946); OGC WFS and WMTS; PMTiles",
            T,
        )
        .field(field(T, "name", BasicType::Text).required())
        .field(field(T, "description", BasicType::MultilineText))
        .field(
            field(T, "source", BasicType::Choice)
                .required()
                .choices(&["places", "geojson", "wfs", "wmts", "pmtiles"]),
        )
        .field(field(T, "url", BasicType::Link))
        .field(
            field(T, "places", BasicType::Reference)
                .target("Place")
                .multiple(),
        )
        .command(find(T))
        .command(
            own(command(T, "create", CommandKind::Create), true)
                .result(field(T, "id", BasicType::Reference).target("MapLayer")),
        )
        .command(own(
            command(T, "change", CommandKind::Change).input(record(T, "MapLayer")),
            false,
        ))
        .command(command(T, "delete", CommandKind::Delete).input(record(T, "MapLayer")))
        // A layer connected to a data source that stays current (Felt).
        .command(command(T, "refresh", CommandKind::Other).input(record(T, "MapLayer")))
        // Several people edit one map live (Felt, uMap): invite a person.
        .command(
            command(T, "invite", CommandKind::Other)
                .input(record(T, "MapLayer"))
                .input(
                    field(T, "person", BasicType::Reference)
                        .target("Person")
                        .required(),
                ),
        )
        // Comment on a map without an editing seat (Felt): an ActivityStreams Note.
        .command(
            command(T, "comment", CommandKind::Other)
                .input(record(T, "MapLayer"))
                .input(field(T, "text", BasicType::MultilineText).required()),
        )
        // Map events and webhooks for embedding (Felt).
        .command(
            command(T, "webhook", CommandKind::Other)
                .input(record(T, "MapLayer"))
                .input(field(T, "url", BasicType::Link).required())
                .input(
                    field(T, "events", BasicType::Choice)
                        .multiple()
                        .choices(&["created", "updated", "deleted"]),
                ),
        )
    }
}
