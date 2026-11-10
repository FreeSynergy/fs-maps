//! `fs-maps-app`: the window of the maps program (ADR 0005, 0009, 0063, 0065, 0066).
//!
//! Data and wiring only: the server behind a socket ([`PlacesRemote`], trait [`PlacesApi`] first), the records and
//! commands of each data type for the generated Admin frontend ([`PlacesBackend`]), what the map component needs
//! ([`map`]) and the window that joins shell, map and frontend ([`window`]). It draws nothing and imports no
//! `blinc_*` crate: everything that shows is a component of `fs-components` (ADR 0066).
//!
//! Pattern: Facade — this file only declares modules and re-exports the public API.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]

mod backend;
mod feature;
pub mod gui;
pub mod map;
mod remote;
pub mod window;
mod words;

pub use backend::{Listener, PlacesBackend};
pub use remote::{ApiError, Place, PlacesApi, PlacesRemote};
pub use words::{English, TITLE};
