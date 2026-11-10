//! The registry of all data types of this program.
//!
//! Pattern: Registry — each type enters through `DescribesType` (`fs_types`).

use fs_types::TypeRegistry;

use super::{MapLayerType, MapTourType, PlaceType, RegionType};

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
