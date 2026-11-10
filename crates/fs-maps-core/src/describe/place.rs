//! `Place`: a named place on Earth.
//!
//! Pattern: Registry entry (`DescribesType`). Standard: the content is a **GeoJSON Feature (RFC 7946)**, the envelope an
//! ActivityStreams `Place` (the contract `places.capnp` carries both); the position field is a **geo: URI (RFC 5870)**,
//! the basic type `place` of ADR 0066. GeoJSON is the format all map tools read and write; RFC 5870 is the one
//! standard text form of a point (with altitude and uncertainty).

use fs_types::{
    BasicType, CommandKind, DescribesType, FieldDescription, TypeDescription,
};

use super::{command, field, find, record};

/// Marker for the description of [`Place`](crate::places) records.
pub struct PlaceType;

const T: &str = "place";

fn values() -> Vec<FieldDescription> {
    vec![
        field(T, "name", BasicType::Text),
        field(T, "position", BasicType::Place),
        field(T, "description", BasicType::MultilineText),
        field(T, "keywords", BasicType::Keywords),
        field(T, "link", BasicType::Link),
    ]
}

fn with_values(mut c: fs_types::CommandDescription, name_required: bool) -> fs_types::CommandDescription {
    for f in values() {
        c = c.input(if f.name == "name" && name_required { f.required() } else { f });
    }
    c
}

impl DescribesType for PlaceType {
    fn describe() -> TypeDescription {
        let mut d = TypeDescription::new(
            "Place",
            "fs-maps",
            "GeoJSON Feature (RFC 7946), ActivityStreams Place, geo: URI (RFC 5870)",
            T,
        );
        for f in values() {
            d = d.field(if f.name == "name" { f.required() } else { f });
        }
        d.command(find(T))
            .command(
                command(T, "within", CommandKind::Find)
                    .input(field(T, "southwest", BasicType::Place).required())
                    .input(field(T, "northeast", BasicType::Place).required()),
            )
            .command(
                with_values(command(T, "create", CommandKind::Create), true)
                    .result(field(T, "id", BasicType::Reference).target("Place")),
            )
            .command(with_values(
                command(T, "change", CommandKind::Change).input(record(T, "Place")),
                false,
            ))
            .command(command(T, "delete", CommandKind::Delete).input(record(T, "Place")))
            .command(
                command(T, "elevationProfile", CommandKind::Other)
                    .input(record(T, "Place"))
                    .result(field(T, "profile", BasicType::MultilineText)),
            )
    }
}
