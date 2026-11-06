//! Shared helpers of the place tests.

#![allow(dead_code)] // each test file uses a different subset

use fs_store::TursoStore;
use serde_json::{Value, json};

pub const ME: &str = "https://example.org/actors/a";

pub async fn store() -> TursoStore {
    TursoStore::open_in_memory().await.unwrap()
}

pub fn point(name: &str, lon: f64, lat: f64) -> Value {
    json!({"type":"Feature","geometry":{"type":"Point","coordinates":[lon, lat]},
           "properties":{"name":name}})
}

pub fn polygon(name: &str, ring: &[[f64; 2]]) -> Value {
    json!({"type":"Feature","geometry":{"type":"Polygon","coordinates":[ring]},
           "properties":{"name":name}})
}

pub fn names(places: &[fs_store::StoredObject]) -> Vec<&str> {
    places.iter().map(|p| p.envelope.name.as_str()).collect()
}
