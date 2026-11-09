//! Test/example helpers: a loopback range server and a PMTiles fixture.
//!
//! Pattern: Fake (test double) — keeps tests and the example off public servers.

mod crafted;
mod fixture;
mod server;

pub use crafted::{archive, big_directory, brotli, directory, gzip, leaf_at_zero};
pub use fixture::{write_fixture, write_labelled};
pub use server::RangeServer;
