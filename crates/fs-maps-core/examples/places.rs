//! Places over an in-memory store: create, find by area (also over the antimeridian) and by name.
//!
//! Pattern: Facade consumer — everything goes through the `Places` trait.

use fs_maps_core::{Area, Paging, PlaceRepository, Places};
use fs_objects::TursoStore;
use serde_json::json;

#[tokio::main]
async fn main() -> fs_maps_core::Result<()> {
    let store = TursoStore::open_in_memory().await?;
    let repo = PlaceRepository::new(&store, "https://example.org/actors/me");
    let places: &dyn Places = &repo;
    for (name, lon, lat) in [
        ("Berlin", 13.4, 52.5),
        ("Suva", 178.44, -18.14),
        ("Apia", -171.76, -13.83),
    ] {
        let feature = json!({"type":"Feature","properties":{"name":name},
            "geometry":{"type":"Point","coordinates":[lon, lat]}});
        places.create(&feature).await?;
    }
    let pacific = Area::new(170.0, -40.0, -170.0, -10.0)?; // west > east: over the antimeridian
    for p in places.within(&pacific, Paging::default()).await? {
        println!("in the Pacific box: {}", p.envelope.name);
    }
    for p in places.find_by_name("ber", Paging::default()).await? {
        println!("named like 'ber': {}", p.envelope.name);
    }
    Ok(())
}
