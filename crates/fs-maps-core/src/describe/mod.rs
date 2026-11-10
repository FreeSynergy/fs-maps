//! The data types of fs-maps, described for a generated frontend (ADR 0066).
//!
//! Pattern: Registry — every data type enters the [`TypeRegistry`] through the trait `DescribesType` of `fs-types`
//! (trait first, ADR 0043). Which standard each type follows is in its file. The descriptions list **every** command:
//! the ones the core has, and the ones the requirements name that it has not yet (`support`).

mod layer;
mod place;
mod region;
mod support;
mod tour;
mod wire;

use fs_types::{BasicType, CommandDescription, CommandKind, FieldDescription, TypeRegistry};

pub use layer::MapLayerType;
pub use place::PlaceType;
pub use region::RegionType;
pub use support::{SUPPORT, Support, unsupported};
pub use tour::MapTourType;
pub use wire::{read_catalog, write_catalog};

/// The registry of all data types of this program.
///
/// # Panics
/// Never for the descriptions in this crate (a test registers and validates every one).
#[must_use]
pub fn registry() -> TypeRegistry {
    let mut r = TypeRegistry::new();
    r.register::<PlaceType>()
        .and_then(|r| r.register::<MapLayerType>())
        .and_then(|r| r.register::<MapTourType>())
        .and_then(|r| r.register::<RegionType>())
        .expect("the descriptions of fs-maps are valid");
    r
}

/// `elevationProfile` -> `elevation-profile`: label keys are lower case (`fs-types` rule).
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// `<type>.<name>`: the label key of a field.
pub(crate) fn field(ty: &str, name: &str, basic: BasicType) -> FieldDescription {
    FieldDescription::new(name, basic).label(&format!("{ty}.{}", kebab(name)))
}

/// The reference to a record of this type, the field that tells a frontend the command works on one record.
pub(crate) fn record(ty: &str, target: &str) -> FieldDescription {
    field(ty, "id", BasicType::Reference)
        .required()
        .target(target)
}

/// `<type>.cmd.<name>`: a command with its label key.
pub(crate) fn command(ty: &str, name: &str, kind: CommandKind) -> CommandDescription {
    CommandDescription::new(name, kind, &format!("{ty}.cmd.{}", kebab(name)))
}

/// The search of the list: a word that the name contains.
pub(crate) fn find(ty: &str) -> CommandDescription {
    command(ty, "find", CommandKind::Find).input(field(ty, "term", BasicType::Text))
}
