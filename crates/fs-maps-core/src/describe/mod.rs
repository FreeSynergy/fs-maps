//! The data types of fs-maps, described for a generated frontend (ADR 0066).
//!
//! Pattern: Registry — every data type enters the [`TypeRegistry`] through the trait `DescribesType` of `fs-types`
//! (trait first, ADR 0043). Which standard each type follows is in its file. The descriptions list **every** command:
//! the ones the core has, and the ones the requirements name that it has not yet (`support`).

mod build;
mod layer;
mod place;
mod region;
mod registry;
mod support;
mod tour;
mod wire;

pub(crate) use build::{command, field, find, record};
pub use layer::MapLayerType;
pub use place::PlaceType;
pub use region::RegionType;
pub use registry::registry;
pub use support::{SUPPORT, Support, unsupported};
pub use tour::MapTourType;
pub use wire::{read_catalog, write_catalog};
