//! `MapTour`: a map with a guided tour or story, ordered stops (Google Earth Voyager).
//!
//! Pattern: Registry entry (`DescribesType`). Standard: an ActivityStreams `OrderedCollection` of `Place` for the
//! stops (the order is the list order), each stop a GeoJSON Feature of the program; AS2 is the standard of lists that
//! carry an order. The core has no tours yet: every command is unsupported.

use fs_types::{BasicType, CommandKind, DescribesType, TypeDescription};

use super::{command, field, find, record};

/// Marker for the description of a map tour.
pub struct MapTourType;

const T: &str = "tour";

impl DescribesType for MapTourType {
    fn describe() -> TypeDescription {
        let own = |c: fs_types::CommandDescription, name_required: bool| {
            let name = field(T, "name", BasicType::Text);
            c.input(if name_required { name.required() } else { name })
                .input(field(T, "description", BasicType::MultilineText))
                .input(field(T, "stops", BasicType::Reference).target("Place").multiple())
        };
        TypeDescription::new("MapTour", "fs-maps", "ActivityStreams OrderedCollection of Place", T)
            .field(field(T, "name", BasicType::Text).required())
            .field(field(T, "description", BasicType::MultilineText))
            .field(field(T, "stops", BasicType::Reference).target("Place").multiple())
            .command(find(T))
            .command(own(command(T, "create", CommandKind::Create), true).result(
                field(T, "id", BasicType::Reference).target("MapTour"),
            ))
            .command(own(
                command(T, "change", CommandKind::Change).input(record(T, "MapTour")),
                false,
            ))
            .command(command(T, "delete", CommandKind::Delete).input(record(T, "MapTour")))
    }
}
