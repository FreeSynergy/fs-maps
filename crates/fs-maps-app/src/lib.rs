//! `fs-maps-app`: the window of the maps program (ADR 0005, 0009, 0063).
//!
//! A proxy to the server ([`PlacesRemote`], behind the trait [`PlacesApi`]), an adapter to the
//! list-and-page workbench of `fs-components` ([`PlacesAdapter`]) and the window ([`gui::run`]). It
//! imports no `blinc_*` crate. There is no map view yet: places are shown as a list and a sheet.
//!
//! Pattern: Facade — this file only declares modules and re-exports the public API.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

mod adapter;
pub mod gui;
mod remote;
mod words;

pub use adapter::PlacesAdapter;
pub use remote::{ApiError, Place, PlacesApi, PlacesRemote};
pub use words::{TITLE, labels};
