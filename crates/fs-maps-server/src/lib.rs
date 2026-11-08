//! `fs-maps-server`: the contract `places.capnp` over a Unix socket, in front of any provider of
//! `Places` and `PlaceFeed` and any `TileSource`. It only translates: no map logic lives here.
//!
//! Interface status: draft (ADR 0045).
//!
//! Pattern: Facade — this file only declares modules and re-exports.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
// The generated `Server` traits are async for every method, also those that never wait.
#![allow(clippy::missing_errors_doc, clippy::unused_async_trait_impl)]

mod errors;
mod feed;
pub mod lease;
mod places_service;
mod program;
mod sources;
mod tiles_service;
mod wire;

pub use places_service::MapsServer;
pub use program::{HELP, ServeError, serve};
pub use sources::{TileConfig, open_tiles};
